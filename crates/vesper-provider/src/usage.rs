//! Provider-neutral, read-only account usage and presentation contract.

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

/// Shared bounded plain-text card for terminal and ACP clients.
pub fn render_usage(context: &UsageContext<'_>, usage: &ProviderUsage) -> String {
    let safe = |value: &str| {
        value
            .chars()
            .filter(|c| !c.is_control())
            .take(160)
            .collect::<String>()
    };
    let mut rows = vec![
        format!("{} · Usage", safe(context.provider)),
        format!("Model:       {}", safe(context.model)),
        format!("Reasoning:   {}", safe(context.reasoning)),
        format!("Permissions: {}", safe(context.permission)),
    ];
    if let Some(auth) = &usage.authentication {
        rows.push(format!("Account:     {}", safe(auth)));
    }
    if let Some(plan) = &usage.plan {
        rows.push(format!("Plan:        {}", safe(plan)));
    }
    if context.context_capacity > 0 {
        let remaining = context
            .context_capacity
            .saturating_sub(context.context_used);
        rows.push(format!(
            "Context:     {:.0}% left ({} / {} tokens used, estimated)",
            remaining as f64 * 100.0 / context.context_capacity as f64,
            context.context_used,
            context.context_capacity
        ));
    } else {
        rows.push("Context:     capacity unavailable".into());
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
                let filled = (left / 10.0).round() as usize;
                format!(
                    "[{}{}] {left:.0}% left",
                    "#".repeat(filled),
                    " ".repeat(10 - filled)
                )
            })
            .unwrap_or_else(|| "remaining unknown".into());
        let reset = window
            .resets_at_unix_ms
            .map(|at| {
                if at <= context.now_unix_ms {
                    " · reset due; refresh usage".into()
                } else {
                    let seconds = (at - context.now_unix_ms) / 1000;
                    format!(
                        " · resets in {}h {:02}m",
                        seconds / 3600,
                        seconds % 3600 / 60
                    )
                }
            })
            .unwrap_or_default();
        rows.push(format!("{}: {value}{reset}", safe(&window.label)));
        if let Some(limit) = window.limit {
            rows.push(format!(
                "  Used: {} · remaining: {} · limit: {limit}",
                window.used.map_or("unknown".into(), |n| n.to_string()),
                window.remaining.map_or("unknown".into(), |n| n.to_string())
            ));
        }
    }
    if let Some(notice) = &usage.notice {
        rows.push(format!("Notice: {}", safe(notice)));
    }
    if !usage.windows.is_empty() {
        rows.push("Limits are a snapshot; run /usage again to refresh.".into());
    }
    let width = rows
        .iter()
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(0)
        .min(100);
    let border = format!("+{}+", "-".repeat(width + 2));
    let mut lines = vec![border.clone()];
    for row in rows {
        let chars: Vec<_> = row.chars().collect();
        for chunk in chars.chunks(width.max(1)) {
            let line: String = chunk.iter().collect();
            lines.push(format!(
                "| {line}{} |",
                " ".repeat(width.saturating_sub(chunk.len()))
            ));
        }
    }
    lines.push(border);
    lines.join("\n")
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
        assert!(card.contains("75% left"));
        assert!(card.contains("0% left"));
        assert!(card.contains("resets in 1h 00m"));
        assert!(card.contains("remaining unknown"));
        assert!(!card.contains('\u{1b}'));
    }
}
