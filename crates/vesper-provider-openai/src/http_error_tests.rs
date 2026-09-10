//! Offline transport regressions: retain safe error classifications, never raw bodies.
use crate::{OpenAiFactory, auth::AuthenticationMode, tests::fixture_request};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use vesper_domain::{ErrorCategory, Retryability};
use vesper_provider::{CancellationSignal, ProviderFactory, ProviderSession};

struct Never;
impl CancellationSignal for Never {
    fn is_cancelled(&self) -> bool {
        false
    }
}

pub(crate) async fn rejected(
    mode: AuthenticationMode,
    status: u16,
    payload: String,
) -> vesper_provider::ProviderError {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            assert!(bytes.len() <= 65536);
            if let Some(start) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = std::str::from_utf8(&bytes[..start]).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= start + 4 + length {
                    break;
                }
            }
        }
        let response = format!(
            "HTTP/1.1 {status} Rejected\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
    });
    let factory = OpenAiFactory::for_loopback(&endpoint, mode).unwrap();
    let session = factory
        .create_session(&OpenAiFactory::default_configuration(), Arc::new(Never))
        .await
        .unwrap();
    let error = match session.start(fixture_request(), Arc::new(Never)).await {
        Ok(_) => panic!("rejected request must not yield a stream"),
        Err(error) => error,
    };
    server.await.unwrap();
    error
}

#[tokio::test]
async fn context_rejection_is_classified_in_both_auth_modes_without_echoing_body() {
    for mode in [AuthenticationMode::ApiKey, AuthenticationMode::ChatGpt] {
        let error = rejected(mode, 400, serde_json::json!({"error": {"code": "context_length_exceeded", "param": "input", "message": "private-prompt-canary", "type": "invalid_request_error"}}).to_string()).await;
        assert_eq!(error.http_status, Some(400));
        assert_eq!(error.info.category, ErrorCategory::ContextLimit);
        assert_eq!(
            error.provider_code.as_ref().map(|c| c.as_str()),
            Some("context_length_exceeded")
        );
        assert_eq!(error.info.retryability, Retryability::Never);
        assert!(!error.continuation_possible);
        let debug = format!("{error:?}");
        assert!(!debug.contains("private-prompt-canary"));
        assert!(debug.contains("input"));
    }
}

#[tokio::test]
async fn unknown_error_fields_are_not_safe_merely_because_they_look_like_identifiers() {
    let error = rejected(AuthenticationMode::ApiKey, 400, serde_json::json!({"error": {"code": "secret_token_canary", "param": "private_project_name", "message": "private-prompt-canary"}}).to_string()).await;
    assert_eq!(error.info.category, ErrorCategory::InvalidRequest);
    assert!(error.provider_code.is_none());
    let serialized = serde_json::to_string(&error).unwrap();
    for secret in [
        "secret_token_canary",
        "private_project_name",
        "private-prompt-canary",
    ] {
        assert!(!serialized.contains(secret));
    }
}

#[tokio::test]
async fn oversized_or_malformed_rejections_preserve_http_status_and_safe_fallback() {
    for payload in [
        "not-json-private-canary".to_owned(),
        format!(
            "{{\"error\":{{\"code\":\"context_length_exceeded\",\"message\":\"{}\"}}}}",
            "x".repeat(70_000)
        ),
    ] {
        let error = rejected(AuthenticationMode::ChatGpt, 400, payload).await;
        assert_eq!(error.http_status, Some(400));
        assert_eq!(error.info.category, ErrorCategory::InvalidRequest);
        assert!(error.provider_code.is_none());
        assert!(!format!("{error:?}").contains("private-canary"));
    }
}

#[tokio::test]
async fn error_body_cannot_override_authentication_status_or_grant_replay() {
    let error = rejected(
        AuthenticationMode::ApiKey,
        403,
        serde_json::json!({"error": {"code": "context_length_exceeded", "param": "input"}})
            .to_string(),
    )
    .await;
    assert_eq!(error.info.category, ErrorCategory::Authentication);
    assert_eq!(error.info.retryability, Retryability::Never);
    assert!(!error.continuation_possible);
}
