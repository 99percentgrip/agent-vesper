use super::*;
use serde_json::{Value, json};
use vesper_domain::*;
use vesper_provider::*;

fn fixture_request() -> ProviderRequest {
    ProviderRequest {
        request_id: ProviderRequestId::new("fixture").unwrap(),
        provider_id: provider_id(),
        model: QualifiedModelId {
            provider_id: provider_id(),
            model_id: ModelId::new(DEFAULT_MODEL).unwrap(),
        },
        endpoint_id: Some(EndpointId::new("xai-responses").unwrap()),
        system_instructions: vec![SystemInstruction {
            content: vec![ContentPart::Text(
                ContentText::new("Use Vesper tools and permissions.").unwrap(),
            )],
            cache_stable: true,
            extensions: Default::default(),
        }],
        messages: vec![ConversationMessage {
            id: MessageId::new("user1").unwrap(),
            role: MessageRole::User,
            content: vec![ContentPart::Text(
                ContentText::new("Read the fixture").unwrap(),
            )],
            extensions: Default::default(),
        }],
        tools: vec![ToolDefinition {
            id: ToolId::new("read-tool").unwrap(),
            harness_name: HarnessToolName::new("read_file").unwrap(),
            provider_name: None,
            description: "Read a confined file".into(),
            input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
            execution_class: ToolExecutionClass::ReadOnly,
            extensions: Default::default(),
            defer_loading: false,
        }],
        tool_choice: ToolChoiceIntent::Auto,
        capabilities: vec![],
        reasoning: None,
        structured_output: StructuredOutputIntent::None,
        sampling: None,
        maximum_output_tokens: Some(4096),
        continuation: None,
        fallback_policy: FallbackPolicy::Strict,
        provider_extensions: None,
    }
}

#[test]
fn descriptor_exposes_separate_session_and_api_billing_modes() {
    let descriptor = XaiFactory::default().descriptor();
    assert_eq!(descriptor.provider_id.as_str(), "xai");
    assert_eq!(descriptor.display_name.as_str(), "xAI / Grok");
    assert_eq!(descriptor.authentication_methods.len(), 2);
    assert_eq!(
        descriptor.authentication_methods[0].method_id.as_str(),
        "xai-grok-session"
    );
    assert_eq!(
        descriptor.authentication_methods[1].method_id.as_str(),
        "xai-api-key"
    );
    assert!(!descriptor.authentication_methods[0].external_runtime_owned);
}

#[test]
fn current_verified_language_model_families_are_explicit() {
    let expected = [
        "grok-4.7",
        "grok-4.6",
        "grok-4.5",
        "grok-4.3",
        "grok-4.20-0309-reasoning",
        "grok-4.20-0309-non-reasoning",
        "grok-4.20-multi-agent-0309",
        "grok-build-0.1",
    ];
    let actual: Vec<_> = XaiCatalog::snapshot()
        .models
        .iter()
        .map(|model| model.model.model_id.as_str().to_owned())
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn reasoning_and_multi_agent_controls_follow_model_evidence() {
    let mut request = fixture_request();
    request.model.model_id = ModelId::new("grok-4.5").unwrap();
    assert!(wire::request(&request, "xhigh").is_err());
    assert!(wire::request(&request, "high").is_ok());

    request.model.model_id = ModelId::new("grok-4.3").unwrap();
    assert!(wire::request(&request, "none").is_ok());

    request.model.model_id = ModelId::new("grok-4.20-0309-non-reasoning").unwrap();
    assert!(wire::request(&request, "none").is_ok());
    assert!(wire::request(&request, "high").is_err());

    request.model.model_id = ModelId::new("grok-4.20-multi-agent-0309").unwrap();
    assert!(wire::request(&request, "high").is_err());
    request.tools.clear();
    request.tool_choice = ToolChoiceIntent::None;
    let body = wire::request(&request, "high").unwrap();
    assert_eq!(body["reasoning"]["effort"], "high");
    assert_eq!(
        XaiCatalog::find("grok-4.20-multi-agent-0309")
            .unwrap()
            .metadata
            .get("xai:reasoning-control-semantics")
            .and_then(serde_json::Value::as_str),
        Some("agent-count")
    );
}

#[test]
fn structured_output_accepts_verified_subset_and_rejects_ambiguous_schemas() {
    let mut request = fixture_request();
    request.structured_output = StructuredOutputIntent::JsonSchema(json!({
        "type":"object",
        "properties":{"item":{"$ref":"#/$defs/item"}},
        "required":["item"],
        "additionalProperties":false,
        "$defs":{"item":{"type":"object","properties":{"name":{"type":"string","maxLength":128}},"required":["name"],"additionalProperties":false}}
    }));
    assert!(wire::request(&request, "high").is_ok());
    for schema in [
        json!({"type":"object","not":{"type":"string"}}),
        json!({"type":"string","pattern":"(?=unsupported-lookahead)"}),
        json!({"anyOf":[]}),
        json!({"type":"array","items":[{"type":"string"}]}),
        json!({"$ref":"#/$defs/loop","$defs":{"loop":{"$ref":"#/$defs/loop"}}}),
    ] {
        request.structured_output = StructuredOutputIntent::JsonSchema(schema);
        assert!(wire::request(&request, "high").is_err());
    }
}

#[test]
fn responses_request_uses_xai_strict_function_schema() {
    let request = fixture_request();
    let body = wire::request(&request, "high").unwrap();
    assert_eq!(body["model"], DEFAULT_MODEL);
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["tools"][0]["name"], "read_file");
    assert_eq!(
        body["tools"][0]["parameters"],
        request.tools[0].input_schema
    );
    assert_eq!(body["tools"][0]["strict"], true);
    assert_eq!(body["include"][0], "reasoning.encrypted_content");
    assert_eq!(body["reasoning"]["effort"], "high");
}

#[test]
fn all_nine_shared_tools_preserve_registry_order_and_identity() {
    let names = [
        "read_file",
        "list_directory",
        "search_files",
        "grep",
        "write_file",
        "edit_file",
        "apply_patch",
        "run_command",
        "update_plan",
    ];
    let mut request = fixture_request();
    request.tools = names
        .iter()
        .map(|name| ToolDefinition {
            id: ToolId::new(*name).unwrap(),
            harness_name: HarnessToolName::new(*name).unwrap(),
            provider_name: None,
            description: format!("{name} fixture"),
            input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
            execution_class: ToolExecutionClass::ReadOnly,
            extensions: Default::default(),
            defer_loading: false,
        })
        .collect();
    let body = wire::request(&request, "high").unwrap();
    let actual: Vec<_> = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(actual, names);
}

#[test]
fn tool_call_usage_and_opaque_reasoning_round_trip() {
    let mut request = fixture_request();
    let mut decoder = wire::Decoder::new(&request);
    decoder.event(json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"call_1","name":"read_file","arguments":""}})).unwrap();
    decoder.event(json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"path\":\"fixture\"}"})).unwrap();
    let events = decoder.event(json!({"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","call_id":"call_1","name":"read_file","arguments":"{\"path\":\"fixture\"}"}})).unwrap();
    let ProviderStreamEvent::ToolCallCompleted(call) = events[0].clone() else {
        panic!("missing call")
    };
    assert_eq!(call.tool_id.as_str(), "read-tool");
    let opaque =
        json!({"type":"reasoning","id":"rs_1","summary":[],"encrypted_content":"opaque-canary"});
    request.messages.push(ConversationMessage {
        id: MessageId::new("assistant1").unwrap(),
        role: MessageRole::Assistant,
        content: vec![
            ContentPart::ProviderOpaque(OpaqueContent {
                provider_id: provider_id(),
                kind: "reasoning".into(),
                data: OpaqueProviderData::new(opaque.clone()).unwrap(),
            }),
            ContentPart::ToolCall(call),
        ],
        extensions: Default::default(),
    });
    let body = wire::request(&request, "high").unwrap();
    assert_eq!(body["input"][1], opaque);
    assert!(!format!("{request:?}").contains("opaque-canary"));
}

#[test]
fn malformed_or_incomplete_tool_call_fails_closed() {
    let mut decoder = wire::Decoder::new(&fixture_request());
    decoder.event(json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_1","name":"read_file","arguments":""}})).unwrap();
    assert!(
        decoder
            .event(json!({"type":"response.completed","response":{}}))
            .is_err()
    );
    assert!(decoder.tool_started);
}

#[test]
fn usage_and_terminal_are_normalized_once() {
    let mut decoder = wire::Decoder::new(&fixture_request());
    let events = decoder.event(json!({"type":"response.completed","response":{"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15,"input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":2}}}})).unwrap();
    let ProviderStreamEvent::Usage(usage) = &events[0] else {
        panic!()
    };
    assert_eq!(usage.cached_input.value, Some(3));
    assert_eq!(usage.reasoning.value, Some(2));
    assert!(matches!(
        events[1],
        ProviderStreamEvent::Completed {
            finish: FinishOutcome::Stop,
            ..
        }
    ));
    assert!(
        decoder
            .event(json!({"type":"response.completed","response":{}}))
            .is_err()
    );
}

#[cfg(feature = "integration-test-harness")]
mod http {
    use super::*;
    use futures_util::StreamExt;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    struct Cancel(AtomicBool);
    impl CancellationSignal for Cancel {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }
    async fn fixture_server(body: String) -> (XaiSession, tokio::task::JoinHandle<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            let (start, length) = loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(i) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&bytes[..i]).unwrap().to_lowercase();
                    assert!(headers.contains("authorization: bearer fixture-xai-key"));
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (k, v) = line.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse().unwrap())
                        })
                        .unwrap();
                    break (i + 4, length);
                }
            };
            while bytes.len() < start + length {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap();
                bytes.extend_from_slice(&buffer[..count]);
            }
            let request = serde_json::from_slice(&bytes[start..start + length]).unwrap();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
            for chunk in body.as_bytes().chunks(3) {
                socket.write_all(chunk).await.unwrap();
            }
            request
        });
        let factory = XaiFactory::for_loopback(&endpoint).unwrap();
        let session = factory
            .create_session(
                &XaiFactory::default_configuration(),
                Arc::new(Cancel(AtomicBool::new(false))),
            )
            .await
            .unwrap();
        (session, server)
    }
    #[tokio::test]
    async fn fragmented_utf8_sse_settles_once() {
        let body = "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\r\n\r\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"Hello 世界\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n".to_owned();
        let (session, server) = fixture_server(body).await;
        let mut stream = session
            .start(fixture_request(), Arc::new(Cancel(AtomicBool::new(false))))
            .await
            .unwrap();
        let mut text = String::new();
        let mut terminals = 0;
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                ProviderStreamEvent::ContentDelta {
                    part: ContentPart::Text(value),
                    ..
                } => text.push_str(value.as_str()),
                ProviderStreamEvent::Completed {
                    finish: FinishOutcome::Stop,
                    ..
                } => terminals += 1,
                _ => {}
            }
        }
        assert_eq!(text, "Hello 世界");
        assert_eq!(terminals, 1);
        assert_eq!(server.await.unwrap()["model"], DEFAULT_MODEL);
    }
    #[tokio::test]
    async fn cancellation_after_headers_settles_without_replay() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut b = [0; 4096];
            let _ = socket.read(&mut b).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        });
        let factory = XaiFactory::for_loopback(&endpoint).unwrap();
        let cancel = Arc::new(Cancel(AtomicBool::new(false)));
        let session = factory
            .create_session(&XaiFactory::default_configuration(), cancel.clone())
            .await
            .unwrap();
        let mut stream = session
            .start(fixture_request(), cancel.clone())
            .await
            .unwrap();
        cancel.0.store(true, Ordering::SeqCst);
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            ProviderStreamEvent::Completed {
                finish: FinishOutcome::Cancelled,
                ..
            }
        ));
        server.abort();
    }

    #[tokio::test]
    async fn oversized_sse_event_settles_as_protocol_error() {
        let body = format!("data: {}\n\n", "x".repeat(wire::MAX_EVENT + 1));
        let (session, server) = fixture_server(body).await;
        let mut stream = session
            .start(fixture_request(), Arc::new(Cancel(AtomicBool::new(false))))
            .await
            .unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            ProviderStreamEvent::Completed {
                finish: FinishOutcome::ProtocolError,
                ..
            }
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn remote_eof_after_visible_output_is_truthful_and_terminal() {
        let body = "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"Visible\"}\n\n".to_owned();
        let (session, server) = fixture_server(body).await;
        let mut stream = session
            .start(fixture_request(), Arc::new(Cancel(AtomicBool::new(false))))
            .await
            .unwrap();
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            ProviderStreamEvent::ContentDelta { .. }
        ));
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            ProviderStreamEvent::Completed {
                finish: FinishOutcome::StreamInterrupted { .. },
                ..
            }
        ));
        assert!(stream.next().await.is_none());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn grok_session_proxy_headers_refresh_once_on_unauthorized_without_api_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let headers = String::from_utf8_lossy(&bytes).to_lowercase();
                assert!(headers.contains("authorization: bearer fixture-xai-key"));
                assert!(headers.contains("x-xai-token-auth: xai-grok-cli"));
                assert!(headers.contains("x-grok-model-override: grok-4.7"));
                requests.push(headers);
                if attempt == 0 {
                    socket.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
                } else {
                    let body = "data: {\"type\":\"response.completed\",\"response\":{}}\n\n";
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                }
            }
            requests.len()
        });
        let credentials = crate::credentials::Credentials::isolated(
            tempfile::tempdir().unwrap().path().join("xai.json"),
        );
        let session = XaiSession::new(
            credentials,
            "high".into(),
            crate::transport::XaiRegion::Global,
        )
        .unwrap()
        .with_test_route(Some(endpoint))
        .with_test_auth_mode(crate::credentials::AuthenticationMode::GrokSession);
        let mut stream = session
            .start(fixture_request(), Arc::new(Cancel(AtomicBool::new(false))))
            .await
            .unwrap();
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            ProviderStreamEvent::Completed {
                finish: FinishOutcome::Stop,
                ..
            }
        ));
        assert_eq!(server.await.unwrap(), 2);
    }
}

#[test]
fn isolated_credential_store_does_not_read_foreign_state() {
    let temp = tempfile::tempdir().unwrap();
    let credentials = crate::credentials::Credentials::isolated(temp.path().join("xai.json"));
    assert!(!credentials.present().unwrap());
    credentials.store_api_key("fixture-key").unwrap();
    assert_eq!(
        credentials.authentication_method().unwrap().as_deref(),
        Some("xai-api-key")
    );
}

#[test]
fn undiscovered_or_unverified_models_fail_before_transport() {
    let session = XaiSession::new(
        crate::credentials::Credentials::isolated(
            tempfile::tempdir().unwrap().path().join("xai.json"),
        ),
        "high".into(),
        crate::transport::XaiRegion::Global,
    )
    .unwrap();
    assert!(session.validate_availability("grok-4.7", false).is_err());
    *session.availability.write().unwrap() = Some(AvailableModels {
        models: vec![XaiCatalog::find("grok-4.7").unwrap()],
        unverified: vec!["future-grok".into()],
        endpoint_excluded: vec![],
    });
    assert!(session.validate_availability("grok-4.7", false).is_ok());
    assert!(session.validate_availability("future-grok", false).is_err());
}
