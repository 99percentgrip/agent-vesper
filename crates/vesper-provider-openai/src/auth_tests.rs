use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Default)]
struct Cancel(AtomicBool);
impl CancellationSignal for Cancel {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

struct Reply {
    path: &'static str,
    status: u16,
    body: String,
    expected: &'static str,
}

async fn server(replies: Vec<Reply>) -> (NativeAuthClient, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        for reply in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let (headers, body_start, size) = loop {
                let mut buf = [0; 1024];
                let count = socket.read(&mut buf).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buf[..count]);
                assert!(request.len() <= MAX_BODY);
                if let Some(index) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8(request[..index].to_vec()).unwrap();
                    let size = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (headers, index + 4, size);
                }
            };
            assert!(headers.starts_with(&format!("POST {} HTTP/1.1", reply.path)));
            while request.len() < body_start + size {
                let mut buf = [0; 1024];
                let count = socket.read(&mut buf).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buf[..count]);
            }
            let body = std::str::from_utf8(&request[body_start..body_start + size]).unwrap();
            assert!(body.contains(reply.expected));
            let response = format!(
                "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.status,
                reply.body.len(),
                reply.body
            );
            // Oversized-response tests intentionally close the socket early.
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    let client = NativeAuthClient {
        client: Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        origin,
    };
    (client, handle)
}

fn user_code() -> Reply {
    Reply {
        path: "/api/accounts/deviceauth/usercode",
        status: 200,
        body: r#"{"device_auth_id":"device-secret","usercode":"ABCD-EFGH","interval":"1"}"#.into(),
        expected: CLIENT_ID,
    }
}

fn tokens() -> SubscriptionTokens {
    SubscriptionTokens {
        access_token: SecretValue::new("old-access"),
        refresh_token: SecretValue::new("old-refresh"),
        id_token: SecretValue::new("old-id"),
    }
}

#[tokio::test]
async fn native_device_flow_polls_exchanges_pkce_and_redacts_all_secrets() {
    let (client, server) = server(vec![
        user_code(),
        Reply { path: "/api/accounts/deviceauth/token", status: 403, body: "{}".into(), expected: "device-secret" },
        Reply { path: "/api/accounts/deviceauth/token", status: 200,
            body: r#"{"authorization_code":"code-secret","code_verifier":"verifier-secret","code_challenge":"challenge"}"#.into(), expected: "ABCD-EFGH" },
        Reply { path: "/oauth/token", status: 200,
            body: r#"{"access_token":"access-secret","refresh_token":"refresh-secret","id_token":"id-secret"}"#.into(), expected: "code_verifier=verifier-secret" },
    ]).await;
    let cancel = Cancel::default();
    let login = client.begin_device_login(&cancel).await.unwrap();
    assert_eq!(login.user_code(), "ABCD-EFGH");
    assert_eq!(
        login.verification_url(),
        format!("{}/codex/device", client.origin)
    );
    assert!(!format!("{login:?}").contains("ABCD-EFGH"));
    assert!(!format!("{login:?}").contains("device-secret"));
    let result = client.complete_device_login(login, &cancel).await.unwrap();
    assert_eq!(result.access_token().expose().as_str(), "access-secret");
    assert_eq!(result.refresh_token().expose().as_str(), "refresh-secret");
    assert_eq!(result.id_token().expose().as_str(), "id-secret");
    assert!(!format!("{result:?}").contains("secret"));
    server.await.unwrap();
}

#[tokio::test]
async fn refresh_preserves_unrotated_fields_and_replaces_rotated_tokens() {
    let (client, server) = server(vec![
        Reply { path: "/oauth/token", status: 200, body: r#"{"access_token":"new-access"}"#.into(), expected: "old-refresh" },
        Reply { path: "/oauth/token", status: 200, body: r#"{"access_token":"newer-access","refresh_token":"new-refresh","id_token":"new-id"}"#.into(), expected: "old-refresh" },
    ]).await;
    let first = client.refresh(&tokens(), &Cancel::default()).await.unwrap();
    assert_eq!(first.access_token().expose().as_str(), "new-access");
    assert_eq!(first.refresh_token().expose().as_str(), "old-refresh");
    let second = client.refresh(&first, &Cancel::default()).await.unwrap();
    assert_eq!(second.refresh_token().expose().as_str(), "new-refresh");
    assert_eq!(second.id_token().expose().as_str(), "new-id");
    server.await.unwrap();
}

#[tokio::test]
async fn expired_refresh_is_terminal_and_does_not_expose_error_body() {
    let (client, server) = server(vec![Reply {
        path: "/oauth/token",
        status: 401,
        body: r#"{"error":"refresh-canary"}"#.into(),
        expected: "old-refresh",
    }])
    .await;
    let error = client
        .refresh(&tokens(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::SessionExpired);
    assert!(!format!("{error:?} {error}").contains("canary"));
    server.await.unwrap();
}

#[tokio::test]
async fn cancellation_and_expiration_prevent_dispatch() {
    let client = NativeAuthClient::new().unwrap();
    let cancel = Cancel(AtomicBool::new(true));
    assert_eq!(
        client.begin_device_login(&cancel).await.unwrap_err(),
        AuthError::Cancelled
    );
    let login = DeviceLogin {
        origin: client.origin.clone(),
        user_code: SecretValue::new("code"),
        device_auth_id: SecretValue::new("id"),
        interval: Duration::from_secs(1),
        deadline: Instant::now(),
    };
    assert_eq!(
        client
            .complete_device_login(login, &Cancel::default())
            .await
            .unwrap_err(),
        AuthError::Timeout
    );
}

#[tokio::test]
async fn cancellation_interrupts_stalled_response() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = NativeAuthClient {
        client: Client::builder().no_proxy().build().unwrap(),
        origin: format!("http://{}", listener.local_addr().unwrap()),
    };
    let cancel = Arc::new(Cancel::default());
    let signal = cancel.clone();
    let server = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        signal.0.store(true, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        client.begin_device_login(cancel.as_ref()),
    )
    .await
    .unwrap();
    assert_eq!(result.unwrap_err(), AuthError::Cancelled);
    server.abort();
}

#[tokio::test]
async fn rejects_oversized_body_and_terminal_escape_in_user_code() {
    for body in [
        "x".repeat(MAX_BODY + 1),
        r#"{"device_auth_id":"id","user_code":"\u001b[31m","interval":1}"#.into(),
    ] {
        let (client, server) = server(vec![Reply {
            body,
            ..user_code()
        }])
        .await;
        assert_eq!(
            client
                .begin_device_login(&Cancel::default())
                .await
                .unwrap_err(),
            AuthError::InvalidResponse
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn unsupported_device_endpoint_is_not_a_fake_success() {
    let (client, server) = server(vec![Reply {
        status: 404,
        body: "{}".into(),
        ..user_code()
    }])
    .await;
    assert_eq!(
        client
            .begin_device_login(&Cancel::default())
            .await
            .unwrap_err(),
        AuthError::DeviceLoginUnavailable
    );
    server.await.unwrap();
}

#[test]
fn malformed_tokens_fail_closed() {
    assert!(decode::<TokenResponse>(br#"{"access_token":""}"#).is_err());
    assert!(decode::<TokenResponse>(br#"{"access_token":"valid","refresh_token":null}"#).is_err());
    assert!(decode::<TokenResponse>(br#"{"access_token":"line\nbreak"}"#).is_err());
    assert_ne!(AuthenticationMode::ApiKey, AuthenticationMode::ChatGpt);
}

#[tokio::test]
async fn excessive_poll_interval_fails_closed() {
    let (client, server) = server(vec![Reply {
        body: r#"{"device_auth_id":"id","user_code":"ABCD","interval":"18446744073709551615"}"#
            .into(),
        ..user_code()
    }])
    .await;
    assert_eq!(
        client
            .begin_device_login(&Cancel::default())
            .await
            .unwrap_err(),
        AuthError::InvalidResponse
    );
    server.await.unwrap();
}

#[tokio::test]
async fn pending_login_expires_during_poll_wait() {
    let (client, server) = server(vec![
        user_code(),
        Reply {
            path: "/api/accounts/deviceauth/token",
            status: 404,
            body: "{}".into(),
            expected: "device-secret",
        },
    ])
    .await;
    let cancel = Cancel::default();
    let mut login = client.begin_device_login(&cancel).await.unwrap();
    login.deadline = Instant::now() + Duration::from_millis(100);
    assert_eq!(
        client
            .complete_device_login(login, &cancel)
            .await
            .unwrap_err(),
        AuthError::Timeout
    );
    server.await.unwrap();
}

#[tokio::test]
async fn redirects_never_forward_authentication_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let location = format!("http://{}/stolen", destination.local_addr().unwrap());
    let mut client = NativeAuthClient::new().unwrap();
    client.origin = format!("http://{}", listener.local_addr().unwrap());
    // Preserve the production client's redirect policy, disable environment proxies.
    client.client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        socket.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    });
    assert_eq!(
        client
            .refresh(&tokens(), &Cancel::default())
            .await
            .unwrap_err(),
        AuthError::Rejected
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), destination.accept())
            .await
            .is_err()
    );
    server.await.unwrap();
}
