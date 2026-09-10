//! Preserve only allowlisted parameter identity even when the service omits a code.
use crate::auth::AuthenticationMode;
use crate::http_error_tests::rejected;
use vesper_domain::{ErrorCategory, Retryability};

#[tokio::test]
async fn null_or_unknown_code_does_not_discard_a_safe_parameter() {
    for mode in [AuthenticationMode::ApiKey, AuthenticationMode::ChatGpt] {
        for code in [
            serde_json::Value::Null,
            serde_json::json!("private-code-canary"),
        ] {
            let error = rejected(
                mode,
                400,
                serde_json::json!({"error": {
                    "code": code, "param": "tools", "message": "private-body-canary"
                }})
                .to_string(),
            )
            .await;
            assert_eq!(
                error.info.diagnostics.fields.get("openai:parameter"),
                Some(&serde_json::json!("tools"))
            );
            assert!(error.provider_code.is_none());
            assert_eq!(error.info.category, ErrorCategory::InvalidRequest);
            assert_eq!(error.info.retryability, Retryability::Never);
            assert!(!error.continuation_possible);
            let encoded = serde_json::to_string(&error).unwrap();
            assert!(!encoded.contains("private-code-canary"));
            assert!(!encoded.contains("private-body-canary"));
        }
    }
}

#[tokio::test]
async fn parameter_allowlist_rejects_paths_prefixes_and_non_strings_in_both_modes() {
    for mode in [AuthenticationMode::ApiKey, AuthenticationMode::ChatGpt] {
        for param in [
            serde_json::json!("tools.private-project-canary"),
            serde_json::json!("tools[0].private-project-canary"),
            serde_json::json!("private-project-canary"),
            serde_json::json!({"tools": "private-project-canary"}),
            serde_json::json!(["tools"]),
            serde_json::Value::Null,
        ] {
            let error = rejected(
                mode,
                400,
                serde_json::json!({"error": {
                    "code": "unsupported_parameter", "param": param,
                    "message": "private-body-canary"
                }})
                .to_string(),
            )
            .await;
            assert!(
                error
                    .info
                    .diagnostics
                    .fields
                    .get("openai:parameter")
                    .is_none()
            );
            assert_eq!(
                error.provider_code.as_ref().map(|code| code.as_str()),
                Some("unsupported_parameter")
            );
            assert_eq!(error.info.category, ErrorCategory::InvalidRequest);
            assert_eq!(error.info.retryability, Retryability::Never);
            assert!(!error.continuation_possible);
            let encoded = serde_json::to_string(&error).unwrap();
            assert!(!encoded.contains("private-project-canary"));
            assert!(!encoded.contains("private-body-canary"));
        }
    }
}
