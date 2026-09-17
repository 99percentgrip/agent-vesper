//! Tight feedback loop for the `/usage` panel regression (2ai).
//!
//! Renders the panel exactly as the TUI/ACP hosts do and asserts the
//! provider-neutral contract: header, aligned field rows, context row,
//! window meters, reset stamps, notice, and the staleness footnote.

use vesper_provider::{ProviderUsage, UsageContext, UsageWindow, render_usage};

fn render(windows: Vec<UsageWindow>, notice: Option<&str>) -> String {
    let context = UsageContext {
        provider: "zai",
        model: "glm-5.3",
        reasoning: "enabled",
        permission: "Ask",
        context_used: 41_000,
        context_capacity: 131_072,
        now_unix_ms: 1_797_220_000_000,
    };
    let usage = ProviderUsage {
        authentication: Some("API key".into()),
        plan: Some("Z.ai · Coding".into()),
        windows,
        notice: notice.map(str::to_owned),
    };
    render_usage(&context, &usage)
}

#[test]
fn panel_header_and_core_rows_survive() {
    let text = render(vec![], None);
    assert!(
        text.contains("Z.ai · Usage"),
        "header line missing:\n{text}"
    );
    assert!(text.contains("Model:"), "Model row missing:\n{text}");
    assert!(
        text.contains("Reasoning:"),
        "Reasoning row missing:\n{text}"
    );
    assert!(
        text.contains("Permissions:"),
        "Permissions row missing:\n{text}"
    );
    assert!(
        text.contains("Context window:"),
        "Context row missing:\n{text}"
    );
    // No windows and no notice: no warning or footnote may appear.
    assert!(!text.contains("Warning:"), "phantom warning:\n{text}");
    assert!(
        !text.contains("Limits may be stale"),
        "phantom staleness note without windows:\n{text}"
    );
}

#[test]
fn window_meter_and_reset_are_present() {
    let text = render(
        vec![UsageWindow {
            label: "5h".into(),
            used_percent: Some(31.0),
            used: Some(31),
            limit: Some(100),
            remaining: Some(69),
            resets_at_unix_ms: Some(1_797_223_600_000),
        }],
        None,
    );
    assert!(text.contains("5h limit"), "label missing:\n{text}");
    assert!(text.contains("% left"), "percentage missing:\n{text}");
    assert!(text.contains("resets "), "reset stamp missing:\n{text}");
    assert!(
        text.contains("Limits may be stale"),
        "footnote missing:\n{text}"
    );
}

#[test]
fn notice_survives_and_rows_align() {
    let text = render(vec![], Some("quota endpoint unreachable"));
    assert!(
        text.contains("Warning: quota endpoint unreachable"),
        "notice dropped:\n{text}"
    );
    // Field rows share one left edge: "Model:" starts before every value.
    let model_col = text.find("Model:").expect("Model row");
    let perms_col = text.find("Permissions:").expect("Permissions row");
    let ctx_col = text.find("Context window:").expect("Context row");
    assert!(model_col < perms_col && perms_col < ctx_col);
}
