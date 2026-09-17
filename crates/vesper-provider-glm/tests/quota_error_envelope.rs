//! 2ai regression: the quota monitor's HTTP-200 error envelope must surface
//! the provider's own failure reason, not "malformed protocol data".
//!
//! Live evidence (2026-09-17): an account with an exhausted/expired coding
//! plan receives HTTP 200 with
//! `{"code":500,"msg":"Internal service error","success":false}` and no
//! `data.limits`. The previous parser classified that as MalformedProtocol,
//! so `/usage` blamed the protocol instead of the account state.

use serde_json::{Value, json};

fn parse(payload: &Value) -> Result<vesper_provider_glm::GlmPlanUsage, String> {
    vesper_provider_glm::__quota_parse_for_tests(payload).map_err(|error| error.to_string())
}

#[test]
fn monitor_error_envelope_surfaces_the_provider_message() {
    let error = parse(&json!({
        "code": 500,
        "msg": "Internal service error",
        "success": false
    }))
    .expect_err("error envelope must not parse as success");
    assert!(
        error.to_lowercase().contains("internal service error"),
        "provider msg must reach the user, got: {error}"
    );
}

#[test]
fn monitor_auth_envelope_surfaces_the_provider_message() {
    let error = parse(&json!({
        "code": 1001,
        "msg": "Authentication parameter not received in Header, unable to authenticate",
        "success": false
    }))
    .expect_err("error envelope must not parse as success");
    assert!(
        error.to_lowercase().contains("authenticate"),
        "auth failure must be named, got: {error}"
    );
}

#[test]
fn monitor_error_without_msg_is_honest_about_the_envelope() {
    let error = parse(&json!({"success": false})).expect_err("must fail");
    assert!(
        error.to_lowercase().contains("quota monitor"),
        "unknown monitor failure must still be attributed to the monitor, got: {error}"
    );
}

#[test]
fn success_envelope_still_parses_limits() {
    let usage = parse(&json!({"data":{"limits":[
        {"type":"TOKENS_LIMIT","usage":100,"currentValue":25,"remaining":75}
    ]}}))
    .expect("valid envelope must still parse");
    assert_eq!(usage.quotas.len(), 1);
}
