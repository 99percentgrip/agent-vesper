use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;

use vesper_domain::ToolProviderScope;
use vesper_mcp::{McpCredentialResolver, McpError, McpServerConfig, McpSession, McpTransport};
use vesper_security::SecretValue;

struct StoredResolver;

impl McpCredentialResolver for StoredResolver {
    fn resolve(&self, reference: &str) -> Option<SecretValue> {
        (reference == "ZAI_API_KEY").then(|| SecretValue::new("stored-zai-canary"))
    }
}

struct MissingResolver(Arc<AtomicUsize>);

impl McpCredentialResolver for MissingResolver {
    fn resolve(&self, _reference: &str) -> Option<SecretValue> {
        self.0.fetch_add(1, Ordering::SeqCst);
        None
    }
}

fn read_request(stream: &mut std::net::TcpStream) -> String {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "request closed before headers");
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "request closed before body");
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn response(status: &str, body: &str, session: bool) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{body}",
        body.len(),
        if session {
            "Mcp-Session-Id: fixture-session\r\n"
        } else {
            ""
        }
    )
}

fn config(url: String) -> McpServerConfig {
    McpServerConfig {
        id: "fixture_http".into(),
        transport: McpTransport::Http,
        command: None,
        args: Vec::new(),
        url: Some(url),
        auth_env: Some("ZAI_API_KEY".into()),
        label: None,
        provider_scope: ToolProviderScope::Any,
        created_at: std::time::SystemTime::UNIX_EPOCH,
    }
}

#[test]
fn adapter_resolver_authenticates_each_http_stage_and_call_name_is_exact() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&requests);
    let server = thread::spawn(move || {
        let initialize = || {
            response(
                "200 OK",
                r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}}}"#,
                true,
            )
        };
        let call_result = || {
            response(
                "200 OK",
                r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"ok"}],"isError":false}}"#,
                false,
            )
        };
        let replies = [
            initialize(),
            response("202 Accepted", "{}", false),
            call_result(),
            initialize(),
            response("202 Accepted", "{}", false),
            call_result(),
        ];
        for reply in replies {
            let (mut stream, _) = listener.accept().unwrap();
            captured.lock().unwrap().push(read_request(&mut stream));
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });

    let session = McpSession::default().with_credential_resolver(Arc::new(StoredResolver));
    let search = session
        .call_tool(
            &config(format!("http://{address}")),
            "webSearchPrime",
            serde_json::json!({"search_query":"rust"}),
        )
        .unwrap();
    let reader = session
        .call_tool(
            &config(format!("http://{address}")),
            "webReader",
            serde_json::json!({"url":"https://example.test/page"}),
        )
        .unwrap();
    assert_eq!(search["isError"], false);
    assert_eq!(reader["isError"], false);
    server.join().unwrap();

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    for request in requests.iter() {
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer stored-zai-canary")
        );
    }
    assert!(requests[2].contains("webSearchPrime"));
    assert!(requests[2].contains(r#""search_query":"rust""#));
    assert!(!requests[2].contains("web_search_prime"));
    assert!(requests[5].contains("webReader"));
    assert!(requests[5].contains(r#""url":"https://example.test/page""#));
}

#[test]
fn http_and_jsonrpc_diagnostics_use_fixed_categories_without_upstream_leakage() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        assert!(request.contains("stored-zai-canary"));
        let body = "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32001,\"message\":\"credential stored-zai-canary failed at https://token.example/?access_token=url-token-canary\\u001b[31m\",\"data\":{\"raw_secret\":\"must-not-leak\"}}}";
        stream
            .write_all(response("403 Forbidden", body, false).as_bytes())
            .unwrap();
    });

    let session = McpSession::default().with_credential_resolver(Arc::new(StoredResolver));
    let error = session
        .tools(&config(format!("http://{address}")))
        .unwrap_err();
    server.join().unwrap();
    assert_eq!(
        error,
        McpError::RemoteResponse {
            http_status: Some(403),
            jsonrpc_code: Some(-32001),
            category: "authorization-or-entitlement-rejected",
        }
    );
    let rendered = error.to_string();
    let debug = format!("{error:?}");
    for output in [&rendered, &debug] {
        assert!(!output.contains("must-not-leak"));
        assert!(!output.contains("stored-zai-canary"));
        assert!(!output.contains("url-token-canary"));
        assert!(!output.contains("token.example"));
        assert!(!output.contains("\u{1b}[31m"));
        assert!(!output.contains(&address.to_string()));
    }
}

#[test]
fn missing_credentials_fail_before_any_http_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let resolutions = Arc::new(AtomicUsize::new(0));
    let session = McpSession::default()
        .with_credential_resolver(Arc::new(MissingResolver(resolutions.clone())));
    let mut server = config(format!("http://{address}"));
    server.auth_env = Some("MISSING_FIXTURE_CREDENTIAL".into());

    let error = session.tools(&server).unwrap_err();
    assert_eq!(error, McpError::Http("auth unavailable"));
    assert_eq!(resolutions.load(Ordering::SeqCst), 1);
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}
