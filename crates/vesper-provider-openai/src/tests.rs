use super::*;
#[allow(unused_imports)]
use serde_json::{Value, json};
use vesper_domain::*;
use vesper_provider::*;

pub(crate) fn fixture_request() -> ProviderRequest {
    ProviderRequest {
        request_id: ProviderRequestId::new("fixture").unwrap(),
        provider_id: provider_id(),
        model: QualifiedModelId {
            provider_id: provider_id(),
            model_id: ModelId::new(DEFAULT_MODEL).unwrap(),
        },
        endpoint_id: Some(EndpointId::new("openai-responses").unwrap()),
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
            input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            execution_class: ToolExecutionClass::ReadOnly,
            provider_scope: Default::default(),
            extensions: Default::default(),
            defer_loading: false,
        }],
        hosted_tools: Vec::new(),
        tool_choice: ToolChoiceIntent::Auto,
        capabilities: vec![],
        reasoning: None,
        structured_output: StructuredOutputIntent::None,
        sampling: None,
        maximum_output_tokens: None,
        continuation: None,
        fallback_policy: FallbackPolicy::Strict,
        cache_routing_key: None,
        provider_extensions: None,
    }
}

#[test]
fn both_auth_modes_use_responses_and_harness_function_schema() {
    for mode in [
        auth::AuthenticationMode::ApiKey,
        auth::AuthenticationMode::ChatGpt,
    ] {
        let request = fixture_request();
        let body = wire::request(&request, mode, "medium").unwrap();
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert!(body.get("messages").is_none());
        assert_eq!(body["tools"][0]["name"], "read_file");
        assert_eq!(
            body["tools"][0]["parameters"],
            request.tools[0].input_schema
        );
        assert_eq!(body["tools"][0]["strict"], false);
        assert_eq!(body["include"][0], "reasoning.encrypted_content");
    }
}

#[test]
fn complete_shared_tool_surface_serializes_identically_in_both_auth_modes() {
    let expected = [
        "apply_patch",
        "edit_file",
        "grep",
        "list_directory",
        "read_file",
        "run_command",
        "search_files",
        "update_plan",
        "write_file",
    ];
    let mut request = fixture_request();
    request.tools = expected
        .iter()
        .map(|name| ToolDefinition {
            id: ToolId::new(*name).unwrap(),
            harness_name: HarnessToolName::new(*name).unwrap(),
            provider_name: None,
            description: format!("{name} fixture description"),
            input_schema: json!({
                "type": "object",
                "properties": {"fixture": {"type": "string"}},
                "required": ["fixture"]
            }),
            execution_class: ToolExecutionClass::ReadOnly,
            provider_scope: Default::default(),
            extensions: Default::default(),
            defer_loading: false,
        })
        .collect();
    for mode in [
        auth::AuthenticationMode::ApiKey,
        auth::AuthenticationMode::ChatGpt,
    ] {
        let body = wire::request(&request, mode, "medium").unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), expected.len());
        for ((wire, definition), name) in tools.iter().zip(&request.tools).zip(expected) {
            assert_eq!(wire["name"], name);
            assert_eq!(wire["description"], definition.description);
            assert_eq!(wire["parameters"], definition.input_schema);
            assert_eq!(wire["strict"], false);
            assert_eq!(definition.id.as_str(), name);
            assert_eq!(definition.harness_name.as_str(), name);
            assert_eq!(definition.provider_name, None);
        }
        assert_eq!(body["tool_choice"], "auto");
    }
}

#[test]
fn every_catalog_model_and_advertised_effort_serializes_natively() {
    assert!(OpenAiCatalog::snapshot().models.len() >= 8);
    for model in OpenAiCatalog::snapshot().models {
        for mode in [
            auth::AuthenticationMode::ApiKey,
            auth::AuthenticationMode::ChatGpt,
        ] {
            for effort in OpenAiCatalog::reasoning_levels_for(model.model.model_id.as_str(), mode) {
                let mut request = fixture_request();
                request.model = model.model.clone();
                let body = wire::request(&request, mode, effort).unwrap();
                assert_eq!(body["model"], model.model.model_id.as_str());
                assert_eq!(body["reasoning"]["effort"], effort);
            }
        }
    }
    assert!(wire::request(&fixture_request(), auth::AuthenticationMode::ApiKey, "none").is_ok());
    assert!(
        wire::request(
            &fixture_request(),
            auth::AuthenticationMode::ChatGpt,
            "none"
        )
        .is_err()
    );
}
#[test]
fn tool_round_trip_preserves_call_identity_and_opaque_reasoning() {
    let mut request = fixture_request();
    let mut decoder = wire::Decoder::new(&request);
    decoder.event(json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"call_1","name":"read_file","arguments":""}})).unwrap();
    decoder.event(json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"path\":"})).unwrap();
    decoder.event(json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"\"fixture\"}"})).unwrap();
    let events=decoder.event(json!({"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","call_id":"call_1","name":"read_file","arguments":"{\"path\":\"fixture\"}"}})).unwrap();
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
    request.messages.push(ConversationMessage {
        id: MessageId::new("tool1").unwrap(),
        role: MessageRole::Tool,
        content: vec![ContentPart::ToolResult(ToolResult {
            id: ToolResultId::new("result1").unwrap(),
            call_id: ToolCallId::new("call_1").unwrap(),
            output: json!("fixture contents"),
            status: ToolResultStatus::Succeeded,
            locations: vec![],
            diff_summary: None,
            extensions: Default::default(),
        })],
        extensions: Default::default(),
    });
    let body = wire::request(&request, auth::AuthenticationMode::ChatGpt, "medium").unwrap();
    assert_eq!(body["input"][1], opaque);
    assert_eq!(body["input"][2]["call_id"], "call_1");
    assert_eq!(body["input"][3]["output"], "fixture contents");
    assert!(!format!("{:?}", request.messages[1]).contains("opaque-canary"));
}
#[test]
fn ambiguous_or_duplicate_calls_never_complete() {
    let request = fixture_request();
    let mut decoder = wire::Decoder::new(&request);
    decoder.event(json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_1","name":"read_file","arguments":""}})).unwrap();
    assert!(
        decoder
            .event(json!({"type":"response.completed","response":{}}))
            .is_err()
    );
    assert!(decoder.tool_started);
    assert!(decoder.event(json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"call_1","name":"read_file","arguments":""}})).is_err());
}
#[test]
fn usage_reasoning_and_finish_are_normalized_once() {
    let mut decoder = wire::Decoder::new(&fixture_request());
    let reasoning=decoder.event(json!({"type":"response.reasoning_summary_text.delta","output_index":0,"delta":"Summary"})).unwrap();
    assert!(matches!(
        reasoning[0],
        ProviderStreamEvent::ReasoningDelta {
            kind: ReasoningKind::Summary,
            ..
        }
    ));
    let events=decoder.event(json!({"type":"response.completed","response":{"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15,"input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":2}}}})).unwrap();
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
#[test]
fn vision_structured_output_and_invalid_controls_are_explicit() {
    let mut request = fixture_request();
    request.messages[0]
        .content
        .push(ContentPart::Image(ImageDescriptor {
            media_type: "image/png".into(),
            source: MediaSource::Reference {
                reference: "data:image/png;base64,iVBORw0KGgo=".into(),
            },
            alt_text: None,
        }));
    request.structured_output = StructuredOutputIntent::JsonSchema(
        json!({"type":"object","properties":{},"additionalProperties":false}),
    );
    let body = wire::request(&request, auth::AuthenticationMode::ApiKey, "medium").unwrap();
    assert_eq!(body["input"][1]["content"][0]["type"], "input_image");
    assert_eq!(body["text"]["format"]["type"], "json_schema");
    assert!(
        wire::request(
            &request,
            auth::AuthenticationMode::ApiKey,
            "invented-effort"
        )
        .is_err()
    );
    request.model.model_id = ModelId::new("invented-model").unwrap();
    assert!(wire::request(&request, auth::AuthenticationMode::ApiKey, "medium").is_err());
}
#[test]
fn catalog_has_no_process_runtime_or_invented_capabilities() {
    for model in OpenAiCatalog::snapshot().models {
        assert!(matches!(
            model.capabilities.process_backed,
            SupportLevel::Unsupported { .. }
        ));
        assert!(matches!(
            model.capabilities.tools,
            SupportLevel::Native { .. }
        ));
        assert_eq!(
            matches!(model.capabilities.vision, SupportLevel::Native { .. }),
            model.model.model_id.as_str() != "gpt-5.3-codex-spark"
        );
    }
    let descriptor = OpenAiFactory::default().descriptor();
    assert_eq!(descriptor.authentication_methods.len(), 2);
    assert!(
        descriptor
            .authentication_methods
            .iter()
            .all(|m| !m.external_runtime_owned)
    );
}

#[test]
fn unsupported_required_capability_fails_before_transport() {
    let mut request = fixture_request();
    request.capabilities.push(CapabilityRequest {
        capability: CapabilityId::new("provider:audio").unwrap(),
        requirement: FeatureRequirement::Require,
        fallback: None,
    });
    for mode in [
        auth::AuthenticationMode::ApiKey,
        auth::AuthenticationMode::ChatGpt,
    ] {
        assert!(wire::request(&request, mode, "medium").is_err());
    }
}

#[cfg(feature = "integration-test-harness")]
mod http {
    use super::*;
    use futures_util::StreamExt;
    use std::sync::Arc;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    struct Never;
    impl CancellationSignal for Never {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
    #[tokio::test]
    async fn native_usage_uses_account_headers_without_inference_and_api_is_explicit() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                let mut buffer = [0; 4096];
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            let headers = String::from_utf8(bytes).unwrap().to_lowercase();
            assert!(headers.starts_with("get /usage http/1.1"));
            assert!(headers.contains("authorization: bearer fixture-openai-key"));
            assert!(headers.contains("chatgpt-account-id: fixture-account"));
            let body = r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":43,"limit_window_seconds":18000,"reset_at":9999999999}}}"#;
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let factory =
            OpenAiFactory::for_loopback(&endpoint, auth::AuthenticationMode::ChatGpt).unwrap();
        let session = factory
            .create_session(&OpenAiFactory::default_configuration(), Arc::new(Never))
            .await
            .unwrap();
        let usage = session.query_usage(Arc::new(Never)).await.unwrap();
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert_eq!(usage.windows[0].used_percent, Some(43.0));
        server.await.unwrap();
        // The listener has closed: API status must not call a subscription or
        // inference endpoint and cannot consume paid generation tokens.
        let factory =
            OpenAiFactory::for_loopback(&endpoint, auth::AuthenticationMode::ApiKey).unwrap();
        let session = factory
            .create_session(&OpenAiFactory::default_configuration(), Arc::new(Never))
            .await
            .unwrap();
        let usage = session.query_usage(Arc::new(Never)).await.unwrap();
        assert!(usage.windows.is_empty());
        assert!(
            usage
                .notice
                .unwrap()
                .contains("Subscription limits do not apply")
        );
    }
    async fn factory_with_chunk_size(
        body: String,
        mode: auth::AuthenticationMode,
        chunk_size: usize,
    ) -> (OpenAiFactory, tokio::task::JoinHandle<Value>) {
        assert!(chunk_size > 0);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/responses", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = vec![];
                let (start, length) = loop {
                    let mut buf = [0; 4096];
                    let count = socket.read(&mut buf).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buf[..count]);
                    if let Some(i) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                        let header = std::str::from_utf8(&bytes[..i]).unwrap();
                        assert!(
                            header
                                .to_lowercase()
                                .contains("authorization: bearer fixture-openai-key")
                        );
                        let length = header
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (i + 4, length);
                    }
                };
                if bytes.starts_with(b"GET /models") {
                    let catalog = if mode == auth::AuthenticationMode::ApiKey {
                        json!({"data":[{"id":"gpt-5.5"}]})
                    } else {
                        json!({"models":[{"slug":"gpt-5.5","visibility":"list"}]})
                    }
                    .to_string();
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{catalog}",catalog.len()).as_bytes()).await.unwrap();
                    continue;
                }
                while bytes.len() < start + length {
                    let mut buf = [0; 4096];
                    let n = socket.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                let request = serde_json::from_slice(&bytes[start..start + length]).unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
                for chunk in body.as_bytes().chunks(chunk_size) {
                    if socket.write_all(chunk).await.is_err() {
                        break;
                    }
                }
                break request;
            }
        });
        let factory = OpenAiFactory::for_loopback(&url, mode).unwrap();
        (factory, server)
    }
    async fn factory(
        body: String,
        mode: auth::AuthenticationMode,
    ) -> (OpenAiFactory, tokio::task::JoinHandle<Value>) {
        factory_with_chunk_size(body, mode, 3).await
    }
    async fn session_with_chunk_size(
        body: String,
        mode: auth::AuthenticationMode,
        chunk_size: usize,
    ) -> (OpenAiSession, tokio::task::JoinHandle<Value>) {
        let (factory, server) = factory_with_chunk_size(body, mode, chunk_size).await;
        let session = factory
            .create_session(&OpenAiFactory::default_configuration(), Arc::new(Never))
            .await
            .unwrap();
        (session, server)
    }
    async fn session(
        body: String,
        mode: auth::AuthenticationMode,
    ) -> (OpenAiSession, tokio::task::JoinHandle<Value>) {
        session_with_chunk_size(body, mode, 3).await
    }
    #[tokio::test]
    async fn memory_extraction_uses_native_auth_and_tool_free_json_in_both_modes() {
        for mode in [
            auth::AuthenticationMode::ApiKey,
            auth::AuthenticationMode::ChatGpt,
        ] {
            let body = "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"{\\\"facts\\\":[]}\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n".to_owned();
            let (factory, server) = factory(body, mode).await;
            assert_eq!(
                factory
                    .extract_memory("Extract JSON facts", "Fixture fact", Arc::new(Never))
                    .await
                    .unwrap(),
                "{\"facts\":[]}"
            );
            let body = server.await.unwrap();
            assert_eq!(body["model"], "gpt-5.5");
            assert_eq!(body["tools"], json!([]));
            assert_eq!(body["text"]["format"]["type"], "json_object");
        }
    }
    #[tokio::test]
    async fn fragmented_utf8_sse_works_in_both_billing_modes() {
        for mode in [
            auth::AuthenticationMode::ApiKey,
            auth::AuthenticationMode::ChatGpt,
        ] {
            let body="data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\r\n\r\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"Hello 世界\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n".to_owned();
            let (session, server) = session(body, mode).await;
            let mut stream = session
                .start(fixture_request(), Arc::new(Never))
                .await
                .unwrap();
            let mut text = String::new();
            let mut terminals = 0;
            while let Some(event) = stream.next().await {
                match event.unwrap() {
                    ProviderStreamEvent::ContentDelta {
                        part: ContentPart::Text(t),
                        ..
                    } => text.push_str(t.as_str()),
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
    }
    #[tokio::test]
    async fn eof_preserves_partial_output_and_marks_ambiguous_tool_without_replay() {
        let body="data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"Visible\"}\n\ndata: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"read_file\",\"arguments\":\"\"}}\n\n".to_owned();
        let (session, server) = session(body, auth::AuthenticationMode::ApiKey).await;
        let mut stream = session
            .start(fixture_request(), Arc::new(Never))
            .await
            .unwrap();
        let mut text = false;
        let mut interrupted = false;
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                ProviderStreamEvent::ContentDelta { .. } => text = true,
                ProviderStreamEvent::Completed {
                    finish:
                        FinishOutcome::StreamInterrupted {
                            tool_call_started: true,
                            ..
                        },
                    ..
                } => interrupted = true,
                ProviderStreamEvent::ToolCallCompleted(_) => panic!("ambiguous tool completed"),
                _ => {}
            }
        }
        assert!(text && interrupted);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn malformed_stream_after_visible_output_has_a_typed_terminal() {
        let body = "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"Keep this\"}\n\ndata: {malformed\n\n".to_owned();
        let (session, server) = session(body, auth::AuthenticationMode::ChatGpt).await;
        let mut stream = session
            .start(fixture_request(), Arc::new(Never))
            .await
            .unwrap();
        let mut visible = false;
        let mut terminal = 0;
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                ProviderStreamEvent::ContentDelta { .. } => visible = true,
                ProviderStreamEvent::Completed {
                    finish: FinishOutcome::StreamInterrupted { .. },
                    ..
                } => terminal += 1,
                _ => {}
            }
        }
        assert!(visible);
        assert_eq!(terminal, 1);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn subscription_metadata_and_reasoning_content_events_are_not_malformed() {
        // Sanitized current upstream subscription shape. Metadata values are
        // intentionally omitted: this regression needs only the documented
        // event discriminants and must never retain account response data.
        let body = concat!(
            "data: {\"type\":\"response.metadata\",\"metadata\":{}}\n\n",
            "data: {\"type\":\"codex.response.metadata\",\"metadata\":{}}\n\n",
            "data: {\"type\":\"response.reasoning_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"bounded reasoning\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"delta\":\"ordinary answer\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
        )
        .to_owned();
        let (session, server) = session(body, auth::AuthenticationMode::ChatGpt).await;
        let mut stream = session
            .start(fixture_request(), Arc::new(Never))
            .await
            .unwrap();
        let mut reasoning = String::new();
        let mut text = String::new();
        let mut terminal = None;
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                ProviderStreamEvent::ReasoningDelta { text: delta, .. } => {
                    reasoning.push_str(delta.as_str());
                }
                ProviderStreamEvent::ContentDelta {
                    part: ContentPart::Text(delta),
                    ..
                } => text.push_str(delta.as_str()),
                ProviderStreamEvent::Completed { finish, .. } => terminal = Some(finish),
                _ => {}
            }
        }
        assert_eq!(reasoning, "bounded reasoning");
        assert_eq!(text, "ordinary answer");
        assert_eq!(terminal, Some(FinishOutcome::Stop));
        server.await.unwrap();
    }

    async fn collect_stream(
        body: String,
        chunk_size: usize,
    ) -> Vec<Result<ProviderStreamEvent, ProviderError>> {
        let (session, server) =
            session_with_chunk_size(body, auth::AuthenticationMode::ApiKey, chunk_size).await;
        let mut stream = session
            .start(fixture_request(), Arc::new(Never))
            .await
            .unwrap();
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }
        server.await.unwrap();
        events
    }

    fn split_metadata_event(data_bytes: usize) -> String {
        let first_prefix = "{\"type\":\"response.metadata\",\"a\":\"";
        let first_suffix = "\",";
        let second_prefix = "\"b\":\"";
        let second_suffix = "\"}";
        let fixed =
            first_prefix.len() + first_suffix.len() + second_prefix.len() + second_suffix.len() + 2; // one accumulator newline per data line
        let padding = data_bytes.checked_sub(fixed).unwrap();
        let first_padding = padding / 2;
        let second_padding = padding - first_padding;
        let first = format!("{first_prefix}{}{first_suffix}", "a".repeat(first_padding));
        let second = format!(
            "{second_prefix}{}{second_suffix}",
            "b".repeat(second_padding)
        );
        assert_eq!(first.len() + second.len() + 2, data_bytes);
        format!("data: {first}\ndata: {second}\n\n")
    }

    #[tokio::test]
    async fn sse_one_byte_http_chunks_preserve_event_boundaries_and_utf8() {
        let body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"one 世界\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
        )
        .to_owned();
        let events = collect_stream(body, 1).await;
        let text = events
            .iter()
            .filter_map(|event| match event.as_ref().unwrap() {
                ProviderStreamEvent::ContentDelta {
                    part: ContentPart::Text(text),
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "one 世界");
        assert!(matches!(
            events.last().unwrap().as_ref().unwrap(),
            ProviderStreamEvent::Completed {
                finish: FinishOutcome::Stop,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn sse_multiple_events_in_one_http_chunk_remain_separate() {
        let body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"first\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"second\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
        )
        .to_owned();
        let events = collect_stream(body.clone(), body.len()).await;
        let deltas = events
            .iter()
            .filter_map(|event| match event.as_ref().unwrap() {
                ProviderStreamEvent::ContentDelta {
                    part: ContentPart::Text(text),
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(deltas, ["first", "second"]);
        assert_eq!(events.len(), 3);
    }

    #[tokio::test]
    async fn sse_crlf_blank_lines_reset_the_data_accumulator() {
        let body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"left\"}\r\n\r\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"right\"}\r\n\r\n",
            "data: {\"type\":\"response.completed\",\"response\":{}}\r\n\r\n",
        )
        .to_owned();
        let events = collect_stream(body, 7).await;
        let text = events
            .iter()
            .filter_map(|event| match event.as_ref().unwrap() {
                ProviderStreamEvent::ContentDelta {
                    part: ContentPart::Text(text),
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "leftright");
        assert_eq!(events.len(), 3);
    }

    #[tokio::test]
    async fn sse_multiline_data_reassembles_only_the_current_json_event() {
        let body = concat!(
            "data: {\"type\":\n",
            "data: \"response.output_text.delta\",\"output_index\":0,\"delta\":\"joined\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
        )
        .to_owned();
        let events = collect_stream(body, 5).await;
        assert!(matches!(
            events.first().unwrap().as_ref().unwrap(),
            ProviderStreamEvent::ContentDelta {
                part: ContentPart::Text(text),
                ..
            } if text.as_str() == "joined"
        ));
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn sse_comments_and_control_fields_never_enter_json_data() {
        let body = concat!(
            ": keepalive\n",
            "id: private-id-that-must-not-enter-json\n",
            "event: response\n",
            "retry: 10\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"clean\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
        )
        .to_owned();
        let events = collect_stream(body, 11).await;
        assert!(matches!(
            events.first().unwrap().as_ref().unwrap(),
            ProviderStreamEvent::ContentDelta {
                part: ContentPart::Text(text),
                ..
            } if text.as_str() == "clean"
        ));
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn sse_eof_discards_an_undelimited_event_without_decoding_or_replay() {
        let body = "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"must not escape\"}\n".to_owned();
        let events = collect_stream(body, 13).await;
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].as_ref().unwrap(),
            ProviderStreamEvent::Completed {
                finish: FinishOutcome::StreamInterrupted {
                    cause: StreamInterruptionCause::RemoteEof,
                    tool_call_started: false,
                },
                ..
            }
        ));
    }

    #[tokio::test]
    async fn sse_aggregate_over_one_mib_across_events_does_not_accumulate() {
        let delta = "x".repeat(4096);
        let mut body = String::new();
        for _ in 0..257 {
            body.push_str("data: ");
            body.push_str(
                &json!({
                    "type": "response.output_text.delta",
                    "output_index": 0,
                    "delta": delta,
                })
                .to_string(),
            );
            body.push_str("\n\n");
        }
        assert!(body.len() > wire::MAX_EVENT);
        body.push_str("data: {\"type\":\"response.completed\",\"response\":{}}\n\n");
        let chunk_size = body.len();
        let events = collect_stream(body, chunk_size).await;
        let mut output_bytes = 0;
        let mut deltas = 0;
        for event in &events {
            match event.as_ref().unwrap() {
                ProviderStreamEvent::ContentDelta {
                    part: ContentPart::Text(text),
                    ..
                } => {
                    deltas += 1;
                    output_bytes += text.as_str().len();
                }
                ProviderStreamEvent::Completed {
                    finish: FinishOutcome::Stop,
                    ..
                } => {}
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert_eq!(deltas, 257);
        assert_eq!(output_bytes, 257 * 4096);
        assert_eq!(events.len(), 258);
    }

    #[tokio::test]
    async fn sse_exact_one_mib_multiline_event_is_accepted_then_cleared() {
        let mut body = split_metadata_event(wire::MAX_EVENT);
        body.push_str("data: {\"type\":\"response.completed\",\"response\":{}}\n\n");
        let events = collect_stream(body, 64 * 1024).await;
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].as_ref().unwrap(),
            ProviderStreamEvent::Completed {
                finish: FinishOutcome::Stop,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn sse_valid_single_line_event_over_one_mib_reports_line_bound() {
        let payload = json!({
            "type": "response.metadata",
            "padding": "x".repeat(wire::MAX_EVENT),
        });
        let body = format!("data: {payload}\n\n");
        let events = collect_stream(body, 64 * 1024).await;
        assert_eq!(events.len(), 1);
        let error = events[0].as_ref().unwrap_err();
        let diagnostic = error
            .info
            .diagnostics
            .fields
            .get("openai:protocol-rejection")
            .unwrap();
        assert_eq!(diagnostic["stage"], "responses-sse-line");
        assert_eq!(diagnostic["field"], "line");
        assert_eq!(diagnostic["observed_bytes_basis"], "sse-line");
        assert!(diagnostic["observed_bytes"].as_u64().unwrap() > wire::MAX_EVENT as u64);
    }

    #[tokio::test]
    async fn sse_multiline_event_over_one_mib_reports_data_bound_not_unknown_type() {
        let body = split_metadata_event(wire::MAX_EVENT + 1);
        let events = collect_stream(body, 64 * 1024).await;
        assert_eq!(events.len(), 1);
        let error = events[0].as_ref().unwrap_err();
        let diagnostic = error
            .info
            .diagnostics
            .fields
            .get("openai:protocol-rejection")
            .unwrap();
        assert_eq!(diagnostic["stage"], "responses-sse-data");
        assert_eq!(diagnostic["field"], "data");
        assert_eq!(diagnostic["observed_bytes_basis"], "sse-data-accumulator");
        assert_eq!(diagnostic["observed_bytes"], wire::MAX_EVENT + 1);
        assert_ne!(diagnostic["stage"], "responses-event");
    }

    #[tokio::test]
    async fn sse_semantic_rejection_can_report_exact_bound_after_bounded_wire_input() {
        // serde_json canonicalizes each `1e100` as `1e+100`. This proves that the
        // decoder's observed_bytes is the reserialized Value size, not the raw
        // SSE accumulator size: a bounded wire event can therefore report the
        // exact 1 MiB bound at the later semantic `type` rejection stage.
        let prefix = r#"{"type":"future.response.event","numbers":[1e100,1e100,1e100,1e100,1e100,1e100],"padding":""#;
        let suffix = r#""}"#;
        // The six-byte `data: ` prefix makes this an exactly-MAX_EVENT SSE
        // line. The six inserted `+` signs then make the parsed Value's
        // canonical serialization exactly MAX_EVENT bytes as well.
        let padding = wire::MAX_EVENT - 6 - prefix.len() - suffix.len();
        let raw = format!("{prefix}{}{suffix}", "x".repeat(padding));
        assert_eq!(raw.len(), wire::MAX_EVENT - 6);
        assert_eq!(
            serde_json::from_str::<Value>(&raw)
                .unwrap()
                .to_string()
                .len(),
            wire::MAX_EVENT
        );

        let events = collect_stream(format!("data: {raw}\n\n"), 64 * 1024).await;
        assert_eq!(events.len(), 1);
        let error = events[0].as_ref().unwrap_err();
        let diagnostic = error
            .info
            .diagnostics
            .fields
            .get("openai:protocol-rejection")
            .unwrap();
        assert_eq!(diagnostic["stage"], "responses-event");
        assert_eq!(diagnostic["event_type"], "<unrecognized>");
        assert_eq!(diagnostic["field"], "type");
        assert_eq!(diagnostic["observed_bytes_basis"], "canonical-json");
        assert_eq!(diagnostic["observed_bytes"], wire::MAX_EVENT);
        assert_eq!(diagnostic["bound"], wire::MAX_EVENT);
    }

    #[tokio::test]
    async fn malformed_after_visible_text_and_tool_start_emits_one_terminal_without_replay() {
        let body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"keep exactly once\"}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"read_file\",\"arguments\":\"\"}}\n\n",
            "data: {malformed\n\n",
        )
        .to_owned();
        let events = collect_stream(body, 17).await;
        let mut text = Vec::new();
        let mut starts = 0;
        let mut calls = 0;
        let mut terminals = 0;
        for event in events {
            match event.unwrap() {
                ProviderStreamEvent::ContentDelta {
                    part: ContentPart::Text(delta),
                    ..
                } => text.push(delta.as_str().to_owned()),
                ProviderStreamEvent::ToolCallStarted { .. } => starts += 1,
                ProviderStreamEvent::ToolCallCompleted(_) => calls += 1,
                ProviderStreamEvent::Completed {
                    finish:
                        FinishOutcome::StreamInterrupted {
                            tool_call_started: true,
                            ..
                        },
                    ..
                } => terminals += 1,
                _ => {}
            }
        }
        assert_eq!(text, ["keep exactly once"]);
        assert_eq!(starts, 1);
        assert_eq!(calls, 0);
        assert_eq!(terminals, 1);
    }
}

#[test]
fn malformed_event_diagnostics_are_bounded_and_secret_safe() {
    let mut decoder = wire::Decoder::new(&fixture_request());
    let canary = "DO_NOT_ECHO_PROTOCOL_SECRET";
    let error = decoder
        .event(json!({
            "type": format!("response.{canary}"),
            "payload": canary,
        }))
        .unwrap_err();
    assert_eq!(error.info.category, ErrorCategory::MalformedProtocol);
    let diagnostic = error
        .info
        .diagnostics
        .fields
        .get("openai:protocol-rejection")
        .unwrap();
    assert_eq!(diagnostic["stage"], "responses-event");
    assert_eq!(diagnostic["event_type"], "<unrecognized>");
    assert_eq!(diagnostic["field"], "type");
    assert_eq!(diagnostic["expected"], "known Responses event type");
    assert_eq!(diagnostic["observed_bytes_basis"], "canonical-json");
    assert!(diagnostic["observed_bytes"].as_u64().unwrap() > 0);
    assert_eq!(diagnostic["bound"], wire::MAX_EVENT);
    let visible = error.info.safe_message.as_str();
    assert_eq!(
        visible,
        "OpenAI returned malformed or oversized Responses data"
    );
    assert!(!visible.contains("responses-event"), "{visible}");
    assert!(!visible.contains("<unrecognized>"), "{visible}");
    assert!(!format!("{error:?}").contains(canary));

    let known_error = wire::Decoder::new(&fixture_request())
        .event(json!({
            "type": "response.output_text.delta",
            "output_index": 0,
        }))
        .unwrap_err();
    assert_eq!(
        known_error.info.safe_message.as_str(),
        "OpenAI returned malformed or oversized Responses data"
    );
    let known_diagnostic = known_error
        .info
        .diagnostics
        .fields
        .get("openai:protocol-rejection")
        .unwrap();
    assert_eq!(known_diagnostic["event_type"], "response.output_text.delta");
    assert_eq!(known_diagnostic["field"], "delta");
    assert_eq!(known_diagnostic["expected"], "bounded text delta");
}

#[test]
fn spark_omits_summary_and_rejects_images() {
    let mut request = fixture_request();
    request.model.model_id = ModelId::new("gpt-5.3-codex-spark").unwrap();
    assert_eq!(
        OpenAiCatalog::context_tokens_for("gpt-5.3-codex-spark"),
        128_000
    );
    let body = wire::request(&request, auth::AuthenticationMode::ChatGpt, "high").unwrap();
    assert!(body["reasoning"].get("summary").is_none());
    request.messages[0]
        .content
        .push(ContentPart::Image(ImageDescriptor {
            media_type: "image/png".into(),
            source: MediaSource::Reference {
                reference: "data:image/png;base64,iVBORw0KGgo=".into(),
            },
            alt_text: None,
        }));
    assert!(wire::request(&request, auth::AuthenticationMode::ChatGpt, "high").is_err());
}
