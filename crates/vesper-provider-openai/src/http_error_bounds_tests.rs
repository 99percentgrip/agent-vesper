//! Local-only hostile response-body bounds; no account or credential storage.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use vesper_domain::{ErrorCategory, Retryability};
use vesper_provider::CancellationSignal;

#[derive(Default)]
struct Cancel(AtomicBool);
impl CancellationSignal for Cancel {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

// Hold an incomplete body open until the caller explicitly releases the server.
async fn incomplete_response() -> (
    reqwest::Response,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let (release, released) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let n = socket.read(&mut bytes).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&bytes[..n]);
            assert!(request.len() <= 8192);
        }
        socket
            .write_all(
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n{",
            )
            .await
            .unwrap();
        let _ = released.await;
    });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(url)
        .send()
        .await
        .unwrap();
    (response, release, server)
}

#[tokio::test]
async fn stalled_rejection_body_has_a_finite_read_budget() {
    let (response, release, server) = incomplete_response().await;
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        super::http_error::rejection(response, &Cancel::default()),
    )
    .await
    .expect("error-body read must be bounded");
    assert_eq!(error.http_status, Some(400));
    assert_eq!(error.info.category, ErrorCategory::InvalidRequest);
    assert!(error.provider_code.is_none());
    assert_eq!(error.info.retryability, Retryability::Never);
    assert!(!error.continuation_possible);
    release.send(()).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn cancellation_interrupts_an_incomplete_error_body() {
    let (response, release, server) = incomplete_response().await;
    let cancel = Arc::new(Cancel::default());
    let signal = cancel.clone();
    let reader =
        tokio::spawn(async move { super::http_error::rejection(response, signal.as_ref()).await });
    cancel.0.store(true, Ordering::SeqCst);
    let error = tokio::time::timeout(Duration::from_secs(1), reader)
        .await
        .expect("cancellation must not wait for body timeout")
        .unwrap();
    assert_eq!(error.http_status, Some(400));
    assert_eq!(error.info.category, ErrorCategory::Cancellation);
    assert_eq!(error.info.retryability, Retryability::Never);
    assert!(!error.continuation_possible);
    release.send(()).unwrap();
    server.await.unwrap();
}
