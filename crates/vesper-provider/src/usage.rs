//! Provider-neutral, read-only account usage and presentation contract.

use chrono::{DateTime, Local, TimeZone};

/// One independently metered window. Missing values never mean zero usage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageWindow {
    pub label: String,
    pub used_percent: Option<f64>,
    pub used: Option<u64>,
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub resets_at_unix_ms: Option<u64>,
}

/// Safe account metadata only: never credentials, raw headers, or JWTs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProviderUsage {
    pub authentication: Option<String>,
    pub plan: Option<String>,
    pub windows: Vec<UsageWindow>,
    pub notice: Option<String>,
}

impl ProviderUsage {
    pub fn unavailable() -> Self {
        Self {
            notice: Some("Account limits are not exposed by this provider.".into()),
            ..Self::default()
        }
    }
}

/// Host-owned status inputs. Context is an estimate, not a quota or billing total.
pub struct UsageContext<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub reasoning: &'a str,
    pub permission: &'a str,
    pub context_used: u64,
    pub context_capacity: u64,
    pub now_unix_ms: u64,
}

/// Shared bounded plain-text status panel for terminal and ACP clients.
pub fn render_usage(context: &UsageContext<'_>, usage: &ProviderUsage) -> String {
    let safe = |value: &str| {
        value
            .chars()
            .filter(|c| !c.is_control())
            .take(160)
            .collect::<String>()
    };
    let mut fields = vec![
        ("Model".to_owned(), safe(context.model)),
        ("Reasoning".to_owned(), safe(context.reasoning)),
        ("Permissions".to_owned(), safe(context.permission)),
    ];
    if let Some(auth) = &usage.authentication {
        fields.push(("Account".to_owned(), safe(auth)));
    }
    if let Some(plan) = &usage.plan {
        fields.push(("Plan".to_owned(), safe(plan)));
    }
    if context.context_capacity > 0 {
        let remaining = context
            .context_capacity
            .saturating_sub(context.context_used);
        fields.push((
            "Context window".to_owned(),
            format!(
                "{:.0}% left ({} used / {}, estimated)",
                remaining as f64 * 100.0 / context.context_capacity as f64,
                compact_count(context.context_used),
                compact_count(context.context_capacity)
            ),
        ));
    } else {
        fields.push(("Context window".to_owned(), "capacity unavailable".into()));
    }
    for window in usage.windows.iter().take(16) {
        let percent = window
            .used_percent
            .filter(|p| p.is_finite() && (0.0..=100.0).contains(p))
            .or_else(|| {
                window.limit.filter(|limit| *limit > 0).and_then(|limit| {
                    window
                        .remaining
                        .map(|n| 100.0 * (1.0 - n.min(limit) as f64 / limit as f64))
                        .or_else(|| {
                            window
                                .used
                                .map(|used| 100.0 * used.min(limit) as f64 / limit as f64)
                        })
                })
            });
        let value = percent
            .map(|used| {
                let left = 100.0 - used;
                let filled = (left * 14.0 / 100.0).round() as usize;
                format!(
                    "[{}{}] {left:.0}% left",
                    "█".repeat(filled),
                    "░".repeat(14 - filled)
                )
            })
            .unwrap_or_else(|| "remaining unknown".into());
        let reset = window
            .resets_at_unix_ms
            .map(|at| {
                format!(
                    " (resets {})",
                    reset_timestamp(at, context.now_unix_ms, &Local)
                )
            })
            .unwrap_or_default();
        let label = safe(&window.label);
        let label = if label.to_ascii_lowercase().contains("limit") {
            label
        } else {
            format!("{label} limit")
        };
        fields.push((label, format!("{value}{reset}")));
    }
    let field_width = fields
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0)
        + 1;
    let mut rows = vec![
        format!("{} · Usage", provider_name(context.provider)),
        String::new(),
    ];
    rows.extend(fields.into_iter().map(|(label, value)| {
        let label = format!("{label}:");
        format!("{label:field_width$}  {value}")
    }));
    if let Some(notice) = &usage.notice {
        rows.push(String::new());
        rows.push(format!("Warning: {}", safe(notice)));
    }
    if !usage.windows.is_empty() {
        rows.push(String::new());
        rows.push("Limits may be stale — run /usage again shortly.".into());
    }
    rows.join("\n")
}

fn provider_name(provider: &str) -> String {
    match provider.to_ascii_lowercase().as_str() {
        "openai" => "OpenAI".into(),
        "zai" | "z.ai" => "Z.ai".into(),
        "lmstudio" | "lm-studio" => "LM Studio".into(),
        _ => provider
            .split(['-', '_'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn compact_count(value: u64) -> String {
    if value < 1_000 {
        return value.to_string();
    }
    let (unit, divisor) = if value >= 1_000_000_000 {
        ('B', 1_000_000_000)
    } else if value >= 1_000_000 {
        ('M', 1_000_000)
    } else {
        ('K', 1_000)
    };
    if value.is_multiple_of(divisor) {
        format!("{}{unit}", value / divisor)
    } else {
        format!("{:.1}{unit}", value as f64 / divisor as f64)
    }
}

// Resolve each instant separately so DST changes do not reuse today's offset.
// Never replace an elapsed provider timestamp with a guessed next reset.
fn reset_timestamp<T: TimeZone>(at: u64, now: u64, timezone: &T) -> String {
    let local = |ms| {
        i64::try_from(ms)
            .ok()
            .and_then(DateTime::from_timestamp_millis)
            .map(|dt| dt.with_timezone(timezone))
    };
    let Some(at) = local(at) else {
        return "time unavailable".into();
    };
    let time = at.time().format("%H:%M");
    if local(now).is_some_and(|now| now.date_naive() == at.date_naive()) {
        time.to_string()
    } else {
        format!("{time} on {}", at.date_naive().format("%-d %b %Y"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn card_distinguishes_unknown_zero_and_reset_and_sanitizes() {
        let context = UsageContext {
            provider: "test",
            model: "model",
            reasoning: "high",
            permission: "ask",
            context_used: 25,
            context_capacity: 100,
            now_unix_ms: 1000,
        };
        let usage = ProviderUsage {
            windows: vec![
                UsageWindow {
                    label: "5h\u{1b}".into(),
                    used_percent: Some(100.0),
                    resets_at_unix_ms: Some(3_601_000),
                    ..Default::default()
                },
                UsageWindow {
                    label: "weekly".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let card = render_usage(&context, &usage);
        assert!(card.starts_with("Test · Usage\n\n"));
        assert!(card.contains("Context window:  75% left (25 used / 100, estimated)"));
        assert!(card.contains("0% left"));
        assert!(card.contains("[░░░░░░░░░░░░░░]"));
        assert!(card.contains(&format!(
            "(resets {})",
            reset_timestamp(3_601_000, 1000, &Local)
        )));
        assert!(card.contains("remaining unknown"));
        assert!(!card.contains('\u{1b}'));
        assert!(!card.contains("+---"));
    }

    #[test]
    fn panel_compacts_large_counts_and_long_reset_intervals() {
        let context = UsageContext {
            provider: "openai",
            model: "gpt-6-astra",
            reasoning: "low",
            permission: "Ask",
            context_used: 4_048,
            context_capacity: 272_000,
            now_unix_ms: 1_000,
        };
        let usage = ProviderUsage {
            windows: vec![UsageWindow {
                label: "Weekly".into(),
                used_percent: Some(30.0),
                resets_at_unix_ms: Some(500_001_000),
                ..Default::default()
            }],
            ..Default::default()
        };
        let panel = render_usage(&context, &usage);
        assert!(panel.contains("OpenAI · Usage"));
        assert!(panel.contains("99% left (4.0K used / 272K, estimated)"));
        assert!(panel.contains("[██████████░░░░] 70% left"));
        assert!(panel.contains(&format!(
            "(resets {})",
            reset_timestamp(500_001_000, 1000, &Local)
        )));
    }

    #[test]
    fn reset_times_preserve_elapsed_and_future_instants_in_local_timezone() {
        let timezone = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let now = 12 * 3600 * 1000; // 20:00 local, 1 Jan 1970.
        let elapsed = (11 * 3600 + 47 * 60) * 1000;
        assert_eq!(reset_timestamp(elapsed, now, &timezone), "19:47");
        assert_eq!(reset_timestamp(now, now, &timezone), "20:00");
        assert_eq!(reset_timestamp(now + 3600 * 1000, now, &timezone), "21:00");
        assert_eq!(
            reset_timestamp(now + 5 * 3600 * 1000, now, &timezone),
            "01:00 on 2 Jan 1970"
        );
        assert_eq!(reset_timestamp(0, now, &timezone), "08:00");
        assert_eq!(
            reset_timestamp(u64::MAX, now, &timezone),
            "time unavailable"
        );
        let west = chrono::FixedOffset::west_opt(5 * 3600).unwrap();
        assert_eq!(reset_timestamp(0, now, &west), "19:00 on 31 Dec 1969");
    }
}
