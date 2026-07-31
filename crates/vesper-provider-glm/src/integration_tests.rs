use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use vesper_domain::{
    CapabilityId, CapabilityRequest, ContentPart, EndpointId, FeatureRequirement, FinishOutcome,
    HarnessToolName, ModelId, ProviderId, ProviderRequestId, QualifiedModelId, ReasoningRetention,
    ToolChoiceIntent, ToolDefinition, ToolExecutionClass, ToolId,
};
use vesper_provider::{
    AuxiliaryRequestIntent, AuxiliaryRequestPort, CancellationSignal, FallbackPolicy,
    ProviderRequest, ProviderSession, ProviderStreamEvent, ReasoningIntent, StructuredOutputIntent,
};
use vesper_security::SecretValue;

use crate::{
    GlmConfig, GlmEndpoint, GlmSession, JitterSource, RetryPolicy, sse::MAX_TOOL_ARGUMENT_BYTES,
};

#[derive(Clone)]
struct Cancellation {
    cancelled: Arc<AtomicBool>,
}

impl Cancellation {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

impl CancellationSignal for Cancellation {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

struct FixedJitter;
impl JitterSource for FixedJitter {
    fn multiplier(&self, _attempt: u32) -> f64 {
        0.75
    }
}

#[derive(Clone)]
struct Part {
    delay: Duration,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct Plan {
    status: u16,
    headers: Vec<(String, String)>,
    header_delay: Duration,
    parts: Vec<Part>,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            header_delay: Duration::ZERO,
            parts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct RecordedRequest {
    path: String,
    authorization: Option<String>,
    body: Value,
}

async fn server(
    plans: Vec<Plan>,
) -> (
    String,
    Arc<Mutex<Vec<RecordedRequest>>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let records = Arc::new(Mutex::new(Vec::new()));
    let task_records = Arc::clone(&records);
    let task = tokio::spawn(async move {
        for plan in plans {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_request(&mut socket).await;
            task_records.lock().unwrap().push(request);
            tokio::time::sleep(plan.header_delay).await;
            let reason = if plan.status == 200 { "OK" } else { "Fixture" };
            let extra = plan
                .headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}\r\n"))
                .collect::<String>();
            let head = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: text/event-stream\r\n{}Connection: close\r\n\r\n",
                plan.status, reason, extra
            );
            if socket.write_all(head.as_bytes()).await.is_err() {
                continue;
            }
            for part in plan.parts {
                tokio::time::sleep(part.delay).await;
                if socket.write_all(&part.bytes).await.is_err() {
                    break;
                }
                let _ = socket.flush().await;
            }
        }
    });
    (format!("http://{address}"), records, task)
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> RecordedRequest {
    let mut bytes = Vec::new();
    let header_end;
    loop {
        let mut buffer = [0_u8; 1024];
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = index + 4;
            break;
        }
    }
    let header = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
    let content_length = header
        .lines()
        .find_map(|line| {
            line.strip_prefix("content-length: ")
                .or_else(|| line.strip_prefix("Content-Length: "))
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while bytes.len() - header_end < content_length {
        let mut buffer = [0_u8; 1024];
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&buffer[..read]);
    }
    let first = header.lines().next().unwrap();
    let path = first.split_whitespace().nth(1).unwrap().to_owned();
    let authorization = header.lines().find_map(|line| {
        line.strip_prefix("authorization: ")
            .or_else(|| line.strip_prefix("Authorization: "))
            .map(str::trim)
            .map(ToOwned::to_owned)
    });
    let body = if content_length == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes[header_end..header_end + content_length]).unwrap()
    };
    RecordedRequest {
        path,
        authorization,
        body,
    }
}

fn sse(value: Value) -> Vec<u8> {
    format!("data: {value}\n").into_bytes()
}

fn chunk(
    content: Option<&str>,
    reasoning: Option<&str>,
    tools: Option<Value>,
    finish: Option<&str>,
) -> Value {
    let mut delta = serde_json::Map::new();
    if let Some(value) = content {
        delta.insert("content".into(), json!(value));
    }
    if let Some(value) = reasoning {
        delta.insert("reasoning_content".into(), json!(value));
    }
    if let Some(value) = tools {
        delta.insert("tool_calls".into(), value);
    }
    let mut choice = serde_json::Map::new();
    choice.insert("index".into(), json!(0));
    choice.insert("delta".into(), Value::Object(delta));
    if let Some(value) = finish {
        choice.insert("finish_reason".into(), json!(value));
    }
    json!({"choices": [Value::Object(choice)]})
}

fn part(bytes: impl Into<Vec<u8>>) -> Part {
    Part {
        delay: Duration::ZERO,
        bytes: bytes.into(),
    }
}

fn delayed(milliseconds: u64, bytes: impl Into<Vec<u8>>) -> Part {
    Part {
        delay: Duration::from_millis(milliseconds),
        bytes: bytes.into(),
    }
}

fn success(content: &str) -> Plan {
    Plan {
        parts: vec![
            part(sse(chunk(Some(content), None, None, Some("stop")))),
            part(b"data: [DONE]\n".to_vec()),
        ],
        ..Plan::default()
    }
}

fn request(with_tools: bool) -> ProviderRequest {
    let mut capabilities = vec![
        capability("provider:reasoning"),
        capability("provider:streamed-reasoning"),
    ];
    let tools = if with_tools {
        capabilities.push(capability("provider:tools"));
        capabilities.push(capability("provider:tool-choice"));
        vec![
            ToolDefinition {
                id: ToolId::new("read_file").unwrap(),
                harness_name: HarnessToolName::new("read_file").unwrap(),
                provider_name: None,
                description: String::new(),
                input_schema: json!({"type":"object"}),
                execution_class: ToolExecutionClass::ReadOnly,
                extensions: Default::default(),
            },
            ToolDefinition {
                id: ToolId::new("write_file").unwrap(),
                harness_name: HarnessToolName::new("write_file").unwrap(),
                provider_name: None,
                description: String::new(),
                input_schema: json!({"type":"object"}),
                execution_class: ToolExecutionClass::Mutating,
                extensions: Default::default(),
            },
        ]
    } else {
        Vec::new()
    };
    ProviderRequest {
        request_id: ProviderRequestId::new("fixture-request").unwrap(),
        provider_id: ProviderId::new("zai").unwrap(),
        model: QualifiedModelId {
            provider_id: ProviderId::new("zai").unwrap(),
            model_id: ModelId::new("glm-5.2").unwrap(),
        },
        endpoint_id: Some(EndpointId::new("zai-custom").unwrap()),
        system_instructions: Vec::new(),
        messages: Vec::new(),
        tools,
        tool_choice: if with_tools {
            ToolChoiceIntent::Auto
        } else {
            ToolChoiceIntent::None
        },
        capabilities,
        reasoning: Some(ReasoningIntent {
            mode: Some(vesper_domain::BoundedString::new("enabled").unwrap()),
            stream_visible: true,
            retention: ReasoningRetention::Persist,
        }),
        structured_output: StructuredOutputIntent::None,
        sampling: None,
        maximum_output_tokens: None,
        continuation: None,
        fallback_policy: FallbackPolicy::Strict,
        provider_extensions: None,
    }
}

fn capability(value: &str) -> CapabilityRequest {
    CapabilityRequest {
        capability: CapabilityId::new(value).unwrap(),
        requirement: FeatureRequirement::Require,
        fallback: None,
    }
}

fn configured_session(url: &str, continuation_limit: u32) -> GlmSession {
    let config = GlmConfig {
        endpoint: GlmEndpoint::custom(url, true, true).unwrap(),
        continuation_limit,
        connect_timeout: Duration::from_secs(1),
        request_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(1),
        ..GlmConfig::default()
    };
    let mut session =
        GlmSession::from_config(config, SecretValue::new("VESPER_SYNTHETIC_CANARY")).unwrap();
    session.retry = RetryPolicy {
        maximum_retries: 3,
        base_delay: Duration::from_millis(1),
        maximum_delay: Duration::from_millis(10),
    };
    session.jitter = Arc::new(FixedJitter);
    session
}

async fn collect(
    session: &GlmSession,
    request: ProviderRequest,
    cancellation: Cancellation,
) -> Vec<Result<ProviderStreamEvent, vesper_provider::ProviderError>> {
    let mut stream = session
        .start(request, Arc::new(cancellation))
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }
    events
}

fn content(events: &[Result<ProviderStreamEvent, vesper_provider::ProviderError>]) -> String {
    events
        .iter()
        .filter_map(|event| match event.as_ref().ok()? {
            ProviderStreamEvent::ContentDelta {
                part: ContentPart::Text(text),
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn reasoning(events: &[Result<ProviderStreamEvent, vesper_provider::ProviderError>]) -> String {
    events
        .iter()
        .filter_map(|event| match event.as_ref().ok()? {
            ProviderStreamEvent::ReasoningDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn terminal(
    events: &[Result<ProviderStreamEvent, vesper_provider::ProviderError>],
) -> Option<&FinishOutcome> {
    events.iter().find_map(|event| match event.as_ref().ok()? {
        ProviderStreamEvent::Completed { finish, .. } => Some(finish),
        _ => None,
    })
}

fn plans(scenario: &str) -> Vec<Plan> {
    match scenario {
        "glm.request-serialization" | "glm.content-only" => vec![success("ok")],
        "glm.reasoning-then-content" => vec![Plan {
            parts: vec![
                part(sse(chunk(None, Some("think"), None, None))),
                part(sse(chunk(Some("answer"), None, None, Some("stop")))),
                part(b"data: [DONE]\n".to_vec()),
            ],
            ..Plan::default()
        }],
        "glm.fragmented-tool-call" => vec![Plan {
            parts: vec![
                part(sse(chunk(
                    None,
                    None,
                    Some(
                        json!([{"index":0,"id":"call-1","function":{"name":"read_file","arguments":"{\"path\":"}}]),
                    ),
                    None,
                ))),
                part(sse(chunk(
                    None,
                    None,
                    Some(json!([{"index":0,"function":{"arguments":"\"x.txt\"}"}}])),
                    None,
                ))),
                part(sse(chunk(None, None, None, Some("tool_calls")))),
                part(b"data: [DONE]\n".to_vec()),
            ],
            ..Plan::default()
        }],
        "glm.interleaved-tool-indexes" => vec![Plan {
            parts: vec![
                part(sse(chunk(
                    None,
                    None,
                    Some(json!([
                        {"index":1,"id":"call-2","function":{"name":"write_file","arguments":"{\"path\":\"b\"}"}}
                    ])),
                    None,
                ))),
                part(sse(chunk(
                    None,
                    None,
                    Some(json!([
                        {"index":0,"id":"call-1","function":{"name":"read_file","arguments":"{\"path\":\"a\"}"}}
                    ])),
                    None,
                ))),
                part(sse(chunk(None, None, None, Some("tool_calls")))),
                part(b"data: [DONE]\n".to_vec()),
            ],
            ..Plan::default()
        }],
        "glm.usage-only-chunk" => vec![Plan {
            parts: vec![
                part(sse(
                    json!({"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":2,"total_tokens":11,"prompt_tokens_details":{"cached_tokens":4}}}),
                )),
                part(sse(chunk(None, None, None, Some("stop")))),
                part(b"data: [DONE]\n".to_vec()),
            ],
            ..Plan::default()
        }],
        "glm.malformed-sse-line" => vec![Plan {
            parts: vec![
                part(b"data: {broken}\n".to_vec()),
                part(success("ok").parts[0].bytes.clone()),
                part(b"data: [DONE]\n".to_vec()),
            ],
            ..Plan::default()
        }],
        "glm.blank-comment-lines" => vec![Plan {
            parts: vec![
                part(b"\n: comment\nevent: ignored\n".to_vec()),
                part(success("ok").parts[0].bytes.clone()),
                part(b"data: [DONE]\n".to_vec()),
            ],
            ..Plan::default()
        }],
        "glm.done-marker" => vec![Plan {
            parts: vec![
                part(sse(chunk(Some("done"), None, None, None))),
                part(b"data: [DONE]\n".to_vec()),
            ],
            ..Plan::default()
        }],
        "glm.terminal-finish-reason" => vec![Plan {
            parts: vec![part(sse(chunk(Some("stop"), None, None, Some("stop"))))],
            ..Plan::default()
        }],
        "glm.incomplete-eof-no-output" => vec![Plan::default(); 4],
        "glm.incomplete-eof-visible-output" => vec![Plan {
            parts: vec![part(sse(chunk(Some("partial"), None, None, None)))],
            ..Plan::default()
        }],
        "glm.retryable-status" => vec![
            Plan {
                status: 503,
                ..Plan::default()
            },
            success("ok"),
        ],
        "glm.non-retryable-status" => vec![Plan {
            status: 401,
            parts: vec![part(b"unauthorized".to_vec())],
            ..Plan::default()
        }],
        "glm.retry-after-numeric" | "glm.retry-after-date" => vec![
            Plan {
                status: 503,
                headers: vec![("Retry-After".into(), "0.001".into())],
                ..Plan::default()
            },
            success("ok"),
        ],
        "glm.cancel-before-headers" => vec![Plan {
            header_delay: Duration::from_millis(500),
            ..success("late")
        }],
        "glm.cancel-mid-stream" => vec![Plan {
            parts: vec![
                part(sse(chunk(Some("first"), None, None, None))),
                delayed(500, sse(chunk(Some("late"), None, None, Some("stop")))),
            ],
            ..Plan::default()
        }],
        "glm.output-length-continuation" => vec![
            Plan {
                parts: vec![
                    part(sse(chunk(Some("part"), None, None, Some("length")))),
                    part(b"data: [DONE]\n".to_vec()),
                ],
                ..Plan::default()
            },
            success("rest"),
        ],
        "glm.continuation-cap" => (0..21)
            .map(|_| Plan {
                parts: vec![
                    part(sse(chunk(Some("x"), None, None, Some("length")))),
                    part(b"data: [DONE]\n".to_vec()),
                ],
                ..Plan::default()
            })
            .collect(),
        _ => panic!("unknown fixture {scenario}"),
    }
}

#[tokio::test]
async fn all_twenty_one_authoritative_glm_scenarios_execute_against_loopback() {
    // Bounded budget: a deadlock (e.g. a mock loopback server left on
    // `accept()` for a connection a cancelled client never makes) must fail
    // the test fast instead of hanging the whole binary and masking other
    // failures. 21 scenarios legitimately need headroom on slow CI.
    tokio::time::timeout(
        Duration::from_secs(30),
        run_all_twenty_one_authoritative_glm_scenarios_execute_against_loopback(),
    )
    .await
    .expect("all_twenty_one exceeded 30s budget — likely a deadlock");
}

async fn run_all_twenty_one_authoritative_glm_scenarios_execute_against_loopback() {
    let fixture_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/provider/glm");
    let mut scenarios = fs::read_dir(&fixture_root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path().join("manifest.json");
            let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            value["scenario_id"].as_str().unwrap().to_owned()
        })
        .collect::<Vec<_>>();
    scenarios.sort();
    assert_eq!(scenarios.len(), 21);

    for scenario in scenarios {
        if scenario == "glm.cancel-before-connect" {
            let cancellation = Cancellation::new();
            cancellation.cancel();
            let config = GlmConfig::default();
            let session = GlmSession::from_config(config, SecretValue::new("synthetic")).unwrap();
            let events = collect(&session, request(false), cancellation).await;
            assert_eq!(terminal(&events), Some(&FinishOutcome::Cancelled));
            continue;
        }
        let expected_plans = plans(&scenario);
        let expected_requests = expected_plans.len();
        let (url, records, server_task) = server(expected_plans).await;
        let session = configured_session(&url, 20);
        let cancellation = Cancellation::new();
        if matches!(
            scenario.as_str(),
            "glm.cancel-before-headers" | "glm.cancel-mid-stream"
        ) {
            let control = cancellation.clone();
            tokio::spawn(async move {
                // 150ms gives the visible "first" chunk ample time to be read on
                // every platform before cancellation fires: the Windows
                // loopback+reqwest round-trip is markedly slower than Linux, and
                // at 25ms the cancel won here before "first" was processed,
                // yielding empty content. The matching mock delays (header_delay
                // / "late" chunk) are pushed to 500ms so the later output still
                // arrives strictly after cancel with a wide safety margin.
                tokio::time::sleep(Duration::from_millis(150)).await;
                control.cancel();
            });
        }
        let with_tools = matches!(
            scenario.as_str(),
            "glm.fragmented-tool-call" | "glm.interleaved-tool-indexes"
        );
        let events = collect(&session, request(with_tools), cancellation).await;
        server_task.abort();
        let captured = records.lock().unwrap().clone();
        assert_eq!(
            captured.len(),
            expected_requests,
            "request count for {scenario}"
        );
        for record in &captured {
            assert_eq!(record.path, "/chat/completions");
            assert_eq!(
                record.authorization.as_deref(),
                Some("Bearer VESPER_SYNTHETIC_CANARY")
            );
        }
        assert!(
            captured
                .iter()
                .all(|request| !request.body.to_string().contains("VESPER_SYNTHETIC_CANARY"))
        );

        match scenario.as_str() {
            "glm.request-serialization" => {
                let fixture: Value = serde_json::from_slice(
                    &fs::read(fixture_root.join("request-serialization/manifest.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    captured[0].body,
                    fixture["expected_network_observations"]["requests"][0]["body"]
                );
            }
            "glm.reasoning-then-content" => {
                assert_eq!(reasoning(&events), "think");
                assert_eq!(content(&events), "answer");
            }
            "glm.content-only"
            | "glm.malformed-sse-line"
            | "glm.blank-comment-lines"
            | "glm.retryable-status"
            | "glm.retry-after-numeric"
            | "glm.retry-after-date" => {
                assert_eq!(content(&events), "ok");
                assert_eq!(terminal(&events), Some(&FinishOutcome::Stop));
            }
            "glm.fragmented-tool-call" => {
                let calls = events
                    .iter()
                    .filter_map(|event| match event.as_ref().ok()? {
                        ProviderStreamEvent::ToolCallCompleted(call) => Some(call),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].arguments, json!({"path":"x.txt"}));
            }
            "glm.interleaved-tool-indexes" => {
                let calls = events
                    .iter()
                    .filter_map(|event| match event.as_ref().ok()? {
                        ProviderStreamEvent::ToolCallCompleted(call) => Some(call),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].arguments, json!({"path":"a"}));
                assert_eq!(calls[1].arguments, json!({"path":"b"}));
            }
            "glm.usage-only-chunk" => {
                let usage = events.iter().find_map(|event| match event.as_ref().ok()? {
                    ProviderStreamEvent::Usage(usage) => Some(usage),
                    _ => None,
                });
                assert_eq!(usage.unwrap().cached_input.value, Some(4));
            }
            "glm.done-marker" => {
                assert_eq!(content(&events), "done");
                assert_eq!(
                    terminal(&events),
                    Some(&FinishOutcome::UnknownProviderValue { raw: String::new() })
                );
            }
            "glm.terminal-finish-reason" => {
                assert_eq!(terminal(&events), Some(&FinishOutcome::Stop));
            }
            "glm.incomplete-eof-no-output" | "glm.non-retryable-status" => {
                assert_eq!(events.iter().filter(|event| event.is_err()).count(), 1);
                assert!(terminal(&events).is_none());
            }
            "glm.incomplete-eof-visible-output" => {
                assert_eq!(content(&events), "partial");
                assert_eq!(
                    terminal(&events),
                    Some(&FinishOutcome::NetworkInterruptionAfterVisibleOutput)
                );
            }
            "glm.cancel-before-headers" => {
                assert_eq!(terminal(&events), Some(&FinishOutcome::Cancelled));
                assert!(content(&events).is_empty());
            }
            "glm.cancel-mid-stream" => {
                assert_eq!(content(&events), "first");
                assert_eq!(terminal(&events), Some(&FinishOutcome::Cancelled));
            }
            "glm.output-length-continuation" => {
                assert_eq!(content(&events), "part\nrest");
                assert_eq!(terminal(&events), Some(&FinishOutcome::Stop));
                assert_eq!(
                    captured[1].body["messages"][1]["content"],
                    "Continue exactly where you left off. Do not repeat or summarize."
                );
            }
            "glm.continuation-cap" => {
                assert_eq!(content(&events), "x\n".repeat(20) + "x");
                assert_eq!(terminal(&events), Some(&FinishOutcome::OutputLimit));
            }
            other => panic!("missing assertion for {other}"),
        }
        assert!(
            events
                .iter()
                .filter(|event| matches!(event, Ok(ProviderStreamEvent::Completed { .. }) | Err(_)))
                .count()
                == 1,
            "one terminal for {scenario}"
        );
    }
}

#[tokio::test]
async fn byte_fragmentation_and_argument_bound_are_enforced() {
    tokio::time::timeout(
        Duration::from_secs(15),
        run_byte_fragmentation_and_argument_bound_are_enforced(),
    )
    .await
    .expect("byte_fragmentation exceeded 15s budget — likely a deadlock");
}

async fn run_byte_fragmentation_and_argument_bound_are_enforced() {
    let payload = sse(chunk(Some("思考"), None, None, Some("stop")));
    let plans = vec![Plan {
        parts: payload.into_iter().map(|byte| part(vec![byte])).collect(),
        ..Plan::default()
    }];
    let (url, _, server_task) = server(plans).await;
    let events = collect(
        &configured_session(&url, 20),
        request(false),
        Cancellation::new(),
    )
    .await;
    server_task.abort();
    assert_eq!(content(&events), "思考");

    let too_large = "x".repeat(MAX_TOOL_ARGUMENT_BYTES + 1);
    let plan = Plan {
        parts: vec![part(sse(chunk(
            None,
            None,
            Some(
                json!([{"index":0,"id":"call","function":{"name":"read_file","arguments":too_large}}]),
            ),
            Some("tool_calls"),
        )))],
        ..Plan::default()
    };
    let (url, _, server_task) = server(vec![plan]).await;
    let events = collect(
        &configured_session(&url, 20),
        request(true),
        Cancellation::new(),
    )
    .await;
    server_task.abort();
    assert_eq!(events.iter().filter(|event| event.is_err()).count(), 1);
}

#[tokio::test]
async fn auxiliary_and_quota_paths_are_bounded_and_independent() {
    tokio::time::timeout(
        Duration::from_secs(15),
        run_auxiliary_and_quota_paths_are_bounded_and_independent(),
    )
    .await
    .expect("auxiliary_and_quota exceeded 15s budget — likely a deadlock");
}

async fn run_auxiliary_and_quota_paths_are_bounded_and_independent() {
    let auxiliary = Plan {
        parts: vec![part(
            br#"{"choices":[{"message":{"content":" bounded answer "}}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}"#
                .to_vec(),
        )],
        ..Plan::default()
    };
    let quota = Plan {
        parts: vec![part(
            br#"{"data":{"limits":[{"type":"TOKENS_LIMIT","usage":100,"currentValue":25,"remaining":75}]}}"#
                .to_vec(),
        )],
        ..Plan::default()
    };
    let (url, records, server_task) = server(vec![auxiliary, quota]).await;
    let session = configured_session(&url, 20);
    let content = session
        .execute_auxiliary(
            AuxiliaryRequestIntent::Compaction,
            request(false),
            Arc::new(Cancellation::new()),
        )
        .await
        .unwrap();
    assert!(matches!(content, ContentPart::Text(ref text) if text.as_str() == "bounded answer"));

    let mut quota_config = session.config().clone();
    quota_config.endpoint = quota_config.endpoint.clone().with_test_quota_support();
    let quota_session =
        GlmSession::from_config(quota_config, SecretValue::new("VESPER_SYNTHETIC_CANARY")).unwrap();
    let usage = quota_session
        .query_plan_usage(Arc::new(Cancellation::new()))
        .await
        .unwrap();
    assert_eq!(usage.quotas[0].remaining, Some(75));
    server_task.abort();

    let captured = records.lock().unwrap();
    assert_eq!(captured[0].path, "/chat/completions");
    assert_eq!(captured[0].body["stream"], false);
    assert_eq!(captured[1].path, "/api/monitor/usage/quota/limit");
    assert_eq!(
        captured[1].authorization.as_deref(),
        Some("VESPER_SYNTHETIC_CANARY")
    );
}

#[tokio::test]
async fn continuation_usage_is_checked_and_cumulative() {
    tokio::time::timeout(
        Duration::from_secs(15),
        run_continuation_usage_is_checked_and_cumulative(),
    )
    .await
    .expect("continuation_usage exceeded 15s budget — likely a deadlock");
}

async fn run_continuation_usage_is_checked_and_cumulative() {
    let first = Plan {
        parts: vec![
            part(sse(
                json!({"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}),
            )),
            part(sse(chunk(Some("first"), None, None, Some("length")))),
            part(b"data: [DONE]\n".to_vec()),
        ],
        ..Plan::default()
    };
    let second = Plan {
        parts: vec![
            part(sse(
                json!({"choices":[],"usage":{"prompt_tokens":4,"completion_tokens":1,"total_tokens":5}}),
            )),
            part(sse(chunk(Some("second"), None, None, Some("stop")))),
            part(b"data: [DONE]\n".to_vec()),
        ],
        ..Plan::default()
    };
    let (url, _, server_task) = server(vec![first, second]).await;
    let events = collect(
        &configured_session(&url, 20),
        request(false),
        Cancellation::new(),
    )
    .await;
    server_task.abort();
    let usages = events
        .iter()
        .filter_map(|event| match event.as_ref().ok()? {
            ProviderStreamEvent::Usage(usage) => Some(usage),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(usages.len(), 2);
    assert_eq!(usages[1].mode, vesper_domain::UsageMode::Cumulative);
    assert_eq!(usages[1].input.value, Some(7));
    assert_eq!(usages[1].output.value, Some(3));
    assert_eq!(usages[1].total.value, Some(10));
}

#[tokio::test]
async fn cancellation_wins_during_backoff_continuation_and_tool_assembly() {
    // This is the historical deadlock source: when the client cancels mid-way
    // through a multi-request continuation/retry, the mock loopback server is
    // left blocked on `accept()` for a connection that never arrives. The
    // server task is now aborted after `collect()` returns, and this budget
    // guarantees the test fails fast instead of hanging the binary.
    tokio::time::timeout(
        Duration::from_secs(15),
        run_cancellation_wins_during_backoff_continuation_and_tool_assembly(),
    )
    .await
    .expect("cancellation_wins exceeded 15s budget — likely a deadlock");
}

async fn run_cancellation_wins_during_backoff_continuation_and_tool_assembly() {
    let cases = [
        vec![Plan {
            status: 503,
            headers: vec![("Retry-After".into(), "1".into())],
            ..Plan::default()
        }],
        vec![
            Plan {
                parts: vec![
                    part(sse(chunk(Some("first"), None, None, Some("length")))),
                    part(b"data: [DONE]\n".to_vec()),
                ],
                ..Plan::default()
            },
            Plan {
                parts: vec![delayed(
                    200,
                    sse(chunk(Some("late"), None, None, Some("stop"))),
                )],
                ..Plan::default()
            },
        ],
        vec![Plan {
            parts: vec![
                part(sse(chunk(
                    None,
                    None,
                    Some(
                        json!([{"index":0,"id":"call-1","function":{"name":"read_file","arguments":"{"}}]),
                    ),
                    None,
                ))),
                delayed(
                    200,
                    sse(chunk(
                        None,
                        None,
                        Some(json!([{"index":0,"function":{"arguments":"}"}}])),
                        Some("tool_calls"),
                    )),
                ),
            ],
            ..Plan::default()
        }],
    ];

    for (index, plans) in cases.into_iter().enumerate() {
        let (url, records, server_task) = server(plans).await;
        let cancellation = Cancellation::new();
        let control = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            control.cancel();
            control.cancel();
        });
        let mut session = configured_session(&url, 20);
        if index == 0 {
            session.retry.maximum_delay = Duration::from_secs(2);
        }
        let events = collect(&session, request(index == 2), cancellation).await;
        server_task.abort();
        assert_eq!(
            terminal(&events),
            Some(&FinishOutcome::Cancelled),
            "case {index}: {events:?}"
        );
        assert!(!content(&events).contains("late"));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Ok(ProviderStreamEvent::Completed { .. })))
                .count(),
            1
        );
        if index == 0 {
            assert_eq!(records.lock().unwrap().len(), 1);
        }
    }
}
