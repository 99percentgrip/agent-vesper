// Keep the provider-neutral error shape at this protocol boundary.
#![allow(clippy::result_large_err)]
use crate::{OpenAiCatalog, auth::AuthenticationMode, error, provider_id};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use vesper_domain::*;
use vesper_provider::*;

pub(crate) const MAX_EVENT: usize = 1_048_576;
const KNOWN_RESPONSE_EVENT_TYPES: &[&str] = &[
    "codex.response.metadata",
    "error",
    "response.completed",
    "response.content_part.added",
    "response.content_part.done",
    "response.created",
    "response.failed",
    "response.function_call_arguments.delta",
    "response.function_call_arguments.done",
    "response.in_progress",
    "response.incomplete",
    "response.metadata",
    "response.output_item.added",
    "response.output_item.done",
    "response.output_text.annotation.added",
    "response.output_text.delta",
    "response.output_text.done",
    "response.queued",
    "response.reasoning_summary_part.added",
    "response.reasoning_summary_part.done",
    "response.reasoning_summary_text.delta",
    "response.reasoning_summary_text.done",
    "response.reasoning_text.delta",
    "response.refusal.delta",
    "response.refusal.done",
    "responsesapi.websocket_timing",
];

fn expected_shape(stage: &str, field: &str) -> &'static str {
    match (stage, field) {
        (_, "type") => "known Responses event type",
        (_, "event-after-terminal") => "no event after terminal response",
        (_, "output_index") => "unsigned 32-bit output index",
        (_, "response.id") => "optional bounded response id string",
        (_, "delta") => "bounded text delta",
        (_, "tool_call_count") => "at most 128 tool calls",
        (_, "item.call_id") => "bounded tool call id string",
        (_, "item.name") => "registered bounded tool name",
        (_, "item.identity") => "matching unique tool call identity",
        (_, "item.arguments") => "JSON object matching the started tool call",
        (_, "extensions") => "bounded OpenAI extension metadata",
        (_, "item.encrypted_content") => "bounded encrypted reasoning object",
        (_, "item.phase") => "commentary or final_answer",
        (_, "unfinished_tool_call") => "all started tool calls completed before terminal",
        ("responses-sse-line", "line") => "single UTF-8 SSE line",
        ("responses-sse-line", "utf8") => "valid UTF-8 SSE line",
        ("responses-sse-data" | "responses-sse-json", "data") => "valid JSON SSE event data",
        (_, "body") => "valid bounded JSON response body",
        (_, "text") => "nonempty bounded text",
        (_, "terminal-event") => "successful terminal event",
        (_, "attempts") => "successful response within retry budget",
        _ => "valid bounded protocol value",
    }
}

fn observed_bytes_basis(stage: &str) -> &'static str {
    match stage {
        "responses-event" => "canonical-json",
        "responses-sse-line" => "sse-line",
        "responses-sse-data" => "sse-data-accumulator",
        "responses-sse-json" => "trimmed-sse-data",
        "responses-body" => "response-body",
        _ => "rejected-value",
    }
}

pub(crate) fn invalid_at(
    stage: &'static str,
    event_type: Option<&str>,
    field: &'static str,
    observed_bytes: Option<usize>,
    bound: Option<usize>,
) -> ProviderError {
    // Diagnostics contain only adapter-owned labels, a sanitized event
    // discriminant, expected shape, and sizes. Never attach the rejected
    // value or response.
    let event_type = match event_type {
        Some(value) if KNOWN_RESPONSE_EVENT_TYPES.contains(&value) => value.to_owned(),
        Some(_) => "<unrecognized>".to_owned(),
        None if stage.starts_with("responses-") => "<unrecognized>".to_owned(),
        None => "not-applicable".to_owned(),
    };
    let expected = expected_shape(stage, field);
    let observed_bytes_basis = observed_bytes_basis(stage);
    // Keep the human-facing message stable and concise. Branch details belong
    // only in the typed, redacted diagnostic below so hosts can present or log
    // them separately without turning internal parser labels into chat prose.
    let mut result = error(
        "OpenAI returned malformed or oversized Responses data",
        ErrorCategory::MalformedProtocol,
        false,
    );
    result
        .info
        .diagnostics
        .fields
        .insert(
            "openai:protocol-rejection",
            json!({
                "stage": stage,
                "event_type": event_type,
                "field": field,
                "expected": expected,
                "observed_bytes": observed_bytes,
                "observed_bytes_basis": observed_bytes_basis,
                "bound": bound,
            }),
        )
        .expect("bounded secret-safe protocol diagnostic");
    result
}
fn unsupported() -> ProviderError {
    error(
        "OpenAI cannot satisfy this request control or content type",
        ErrorCategory::UnsupportedCapability,
        false,
    )
}

pub(crate) fn request(
    request: &ProviderRequest,
    mode: AuthenticationMode,
    default_effort: &str,
) -> Result<Value, ProviderError> {
    if request.provider_id != provider_id()
        || request.model.provider_id != provider_id()
        || OpenAiCatalog::find(request.model.model_id.as_str()).is_none()
    {
        return Err(unsupported());
    }
    if !request.hosted_tools.is_empty()
        || request.continuation.is_some()
        || request.sampling.is_some()
        || request.provider_extensions.is_some()
    {
        return Err(unsupported());
    }
    if request
        .maximum_output_tokens
        .is_some_and(|n| n == 0 || n > 128_000)
    {
        return Err(unsupported());
    }
    let model = request.model.model_id.as_str();
    let capabilities = OpenAiCatalog::find(model)
        .ok_or_else(unsupported)?
        .capabilities;
    for intent in &request.capabilities {
        if matches!(
            capabilities.resolve(intent.capability.as_str(), intent.requirement, false),
            CapabilityResolution::Reject | CapabilityResolution::Fallback
        ) {
            return Err(unsupported());
        }
    }
    let effort = request
        .reasoning
        .as_ref()
        .and_then(|r| r.mode.as_ref())
        .map(|s| s.as_str())
        .unwrap_or(default_effort);
    if !OpenAiCatalog::reasoning_levels_for(model, mode).contains(&effort) {
        return Err(unsupported());
    }
    let mut instructions = Vec::new();
    for instruction in &request.system_instructions {
        for part in &instruction.content {
            if let ContentPart::Text(text) = part {
                instructions.push(text.as_str());
            } else {
                return Err(unsupported());
            }
        }
    }
    let mut input: Vec<Value> = vec![];
    let mut image_count = 0;
    for message in &request.messages {
        let assistant = message.role == MessageRole::Assistant;
        let role = match message.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "user",
            _ => return Err(unsupported()),
        };
        // Never invent linkage for older imported tool text with no call ID.
        if message.role == MessageRole::Tool
            && !message
                .content
                .iter()
                .any(|part| matches!(part, ContentPart::ToolResult(_)))
        {
            return Err(unsupported());
        }
        for part in &message.content {
            match part {
                ContentPart::Text(_) if message.role == MessageRole::Tool => {},
                ContentPart::Text(text)=>input.push(json!({"role":role,"content":[{"type":if assistant{"output_text"}else{"input_text"},"text":text.as_str()}]})),
                ContentPart::Image(image)=>{
                    image_count+=1;
                    if model == "gpt-5.3-codex-spark" || assistant || image_count>50 || !["image/png","image/jpeg","image/webp"].contains(&image.media_type.as_str()){return Err(unsupported());}
                    let MediaSource::Reference{reference}=&image.source else{return Err(unsupported());};
                    let url=url::Url::parse(reference).map_err(|_|unsupported())?;
                    if !["https","data"].contains(&url.scheme()) || reference.len()>8*MAX_EVENT {return Err(unsupported());}
                    if url.scheme()=="data" && !reference.starts_with(&format!("data:{};base64,",image.media_type)){return Err(unsupported());}
                    input.push(json!({"role":"user","content":[{"type":"input_image","image_url":reference}]}));
                }
                ContentPart::ToolCall(call)=>{
                    let name=call.extensions.get("openai:name").and_then(Value::as_str).or_else(||request.tools.iter().find(|t|t.id==call.tool_id).map(tool_name)).unwrap_or(call.tool_id.as_str());
                    input.push(json!({"type":"function_call","call_id":call.id.as_str(),"name":name,"arguments":call.arguments.to_string()}));
                }
                ContentPart::ToolResult(result)=>input.push(json!({"type":"function_call_output","call_id":result.call_id.as_str(),"output":result.output.as_str().map(str::to_owned).unwrap_or_else(||result.output.to_string())})),
                ContentPart::Reasoning(reasoning)=>{
                    if reasoning.retention!=ReasoningRetention::Disabled && let Some(opaque)=&reasoning.opaque { append_opaque(&mut input,opaque)?; }
                }
                ContentPart::ProviderOpaque(opaque)=>append_opaque(&mut input,opaque)?,
                ContentPart::EmbeddedContext(context) if !context.provider_visible=>{},
                _=>return Err(unsupported()),
            }
        }
    }
    let mut names = BTreeSet::new();
    let mut tools = vec![];
    for tool in &request.tools {
        let name = tool_name(tool);
        if name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || !names.insert(name)
            || tools.len() >= 128
        {
            return Err(unsupported());
        }
        tools.push(json!({"type":"function","name":name,"description":tool.description,"parameters":tool.input_schema,"strict":false}));
    }
    let choice = match &request.tool_choice {
        ToolChoiceIntent::Auto => json!("auto"),
        ToolChoiceIntent::None => json!("none"),
        ToolChoiceIntent::Required if !tools.is_empty() => json!("required"),
        ToolChoiceIntent::Named(id) => {
            json!({"type":"function","name":tool_name(request.tools.iter().find(|t|&t.id==id).ok_or_else(unsupported)?)})
        }
        _ => return Err(unsupported()),
    };
    let mut body = json!({"model":model,"instructions":instructions.join("\n\n"),"input":input,"tools":tools,"tool_choice":choice,"parallel_tool_calls":true,"store":false,"stream":true,"include":["reasoning.encrypted_content"],"reasoning":{"effort":effort,"summary":"auto"}});
    if model == "gpt-5.3-codex-spark" {
        body["reasoning"]
            .as_object_mut()
            .expect("object")
            .remove("summary");
    }
    if mode == AuthenticationMode::ApiKey
        && let Some(max) = request.maximum_output_tokens
    {
        body["max_output_tokens"] = json!(max);
    }
    match &request.structured_output {
        StructuredOutputIntent::None => {}
        StructuredOutputIntent::JsonObject => {
            body["text"] = json!({"format":{"type":"json_object"}})
        }
        StructuredOutputIntent::JsonSchema(schema) => {
            body["text"] = json!({"format":{"type":"json_schema","name":"vesper_output","strict":true,"schema":schema}})
        }
        _ => return Err(unsupported()),
    }
    if body.to_string().len() > 32 * MAX_EVENT {
        return Err(unsupported());
    }
    Ok(body)
}
fn append_opaque(input: &mut Vec<Value>, opaque: &OpaqueContent) -> Result<(), ProviderError> {
    if opaque.provider_id != provider_id() {
        return Err(unsupported());
    }
    match opaque.kind.as_str() {
        "reasoning"
            if opaque.data.expose().get("type").and_then(Value::as_str) == Some("reasoning") =>
        {
            input.push(opaque.data.expose().clone())
        }
        "message-phase" => {
            if let Some(last) = input.last_mut() {
                last["phase"] = opaque.data.expose().clone();
            }
        }
        _ => return Err(unsupported()),
    }
    Ok(())
}
pub(crate) fn tool_name(tool: &ToolDefinition) -> &str {
    tool.provider_name
        .as_ref()
        .map(|p| p.as_str())
        .unwrap_or(tool.harness_name.as_str())
}

struct Call {
    id: ToolCallId,
    name: String,
    args: String,
    done: bool,
}
pub(crate) struct Decoder {
    calls: BTreeMap<u32, Call>,
    names: BTreeMap<String, ToolId>,
    pub visible: bool,
    pub tool_started: bool,
    pub terminal: bool,
    pub output_bytes: u64,
    retention: ReasoningRetention,
}
impl Decoder {
    pub fn new(request: &ProviderRequest) -> Self {
        Self {
            calls: BTreeMap::new(),
            names: request
                .tools
                .iter()
                .map(|t| (tool_name(t).to_owned(), t.id.clone()))
                .collect(),
            visible: false,
            tool_started: false,
            terminal: false,
            output_bytes: 0,
            retention: request
                .reasoning
                .as_ref()
                .map(|r| r.retention)
                .unwrap_or(ReasoningRetention::SessionOnly),
        }
    }
    pub fn event(&mut self, value: Value) -> Result<Vec<ProviderStreamEvent>, ProviderError> {
        let observed_bytes = value.to_string().len();
        let raw_kind = value.get("type").and_then(Value::as_str);
        if self.terminal {
            return Err(invalid_at(
                "responses-event",
                raw_kind,
                "event-after-terminal",
                Some(observed_bytes),
                Some(MAX_EVENT),
            ));
        }
        let mut events = vec![];
        let kind = raw_kind.ok_or_else(|| {
            invalid_at(
                "responses-event",
                None,
                "type",
                Some(observed_bytes),
                Some(MAX_EVENT),
            )
        })?;
        let index = || {
            value
                .get("output_index")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| {
                    invalid_at(
                        "responses-event",
                        Some(kind),
                        "output_index",
                        Some(observed_bytes),
                        Some(MAX_EVENT),
                    )
                })
        };
        match kind {
            "response.created" => events.push(ProviderStreamEvent::ResponseStarted {
                response_id: value["response"]["id"]
                    .as_str()
                    .map(ProviderResponseId::new)
                    .transpose()
                    .map_err(|_| {
                        invalid_at(
                            "responses-event",
                            Some(kind),
                            "response.id",
                            Some(observed_bytes),
                            Some(MAX_EVENT),
                        )
                    })?,
                metadata: Default::default(),
            }),
            "response.output_text.delta" | "response.refusal.delta" => {
                let delta = value["delta"].as_str().ok_or_else(|| {
                    invalid_at(
                        "responses-event",
                        Some(kind),
                        "delta",
                        Some(observed_bytes),
                        Some(MAX_EVENT),
                    )
                })?;
                self.output_bytes = self.output_bytes.saturating_add(delta.len() as u64);
                self.visible |= !delta.is_empty();
                events.push(ProviderStreamEvent::ContentDelta {
                    stream_id: BoundedString::new(format!("text-{}", index()?)).map_err(|_| {
                        invalid_at(
                            "responses-event",
                            Some(kind),
                            "output_index",
                            Some(observed_bytes),
                            Some(MAX_EVENT),
                        )
                    })?,
                    part: ContentPart::Text(ContentText::new(delta).map_err(|_| {
                        invalid_at(
                            "responses-event",
                            Some(kind),
                            "delta",
                            Some(delta.len()),
                            Some(MAX_EVENT),
                        )
                    })?),
                });
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                let delta = value["delta"].as_str().ok_or_else(|| {
                    invalid_at(
                        "responses-event",
                        Some(kind),
                        "delta",
                        Some(observed_bytes),
                        Some(MAX_EVENT),
                    )
                })?;
                self.visible |= !delta.is_empty();
                events.push(ProviderStreamEvent::ReasoningDelta {
                    stream_id: BoundedString::new(format!("reasoning-{}", index()?)).map_err(
                        |_| {
                            invalid_at(
                                "responses-event",
                                Some(kind),
                                "output_index",
                                Some(observed_bytes),
                                Some(MAX_EVENT),
                            )
                        },
                    )?,
                    text: ContentText::new(delta).map_err(|_| {
                        invalid_at(
                            "responses-event",
                            Some(kind),
                            "delta",
                            Some(delta.len()),
                            Some(MAX_EVENT),
                        )
                    })?,
                    kind: if kind == "response.reasoning_text.delta" {
                        ReasoningKind::ProviderVisible
                    } else {
                        ReasoningKind::Summary
                    },
                    retention: self.retention,
                });
            }
            "response.output_item.added" if value["item"]["type"] == "function_call" => {
                self.tool_started = true;
                if self.calls.len() >= 128 {
                    return Err(invalid_at(
                        "responses-event",
                        Some(kind),
                        "tool_call_count",
                        Some(self.calls.len().saturating_add(1)),
                        Some(128),
                    ));
                }
                let item = &value["item"];
                let raw_id = item["call_id"].as_str().ok_or_else(|| {
                    invalid_at(
                        "responses-event",
                        Some(kind),
                        "item.call_id",
                        Some(observed_bytes),
                        Some(MAX_EVENT),
                    )
                })?;
                let id = ToolCallId::new(raw_id).map_err(|_| {
                    invalid_at(
                        "responses-event",
                        Some(kind),
                        "item.call_id",
                        Some(raw_id.len()),
                        Some(MAX_EVENT),
                    )
                })?;
                let name = item["name"]
                    .as_str()
                    .ok_or_else(|| {
                        invalid_at(
                            "responses-event",
                            Some(kind),
                            "item.name",
                            Some(observed_bytes),
                            Some(MAX_EVENT),
                        )
                    })?
                    .to_owned();
                if !self.names.contains_key(&name) || self.calls.values().any(|c| c.id == id) {
                    return Err(invalid_at(
                        "responses-event",
                        Some(kind),
                        "item.identity",
                        Some(observed_bytes),
                        Some(MAX_EVENT),
                    ));
                }
                let idx = index()?;
                let args = item["arguments"].as_str().unwrap_or("").to_owned();
                if self
                    .calls
                    .insert(
                        idx,
                        Call {
                            id: id.clone(),
                            name: name.clone(),
                            args,
                            done: false,
                        },
                    )
                    .is_some()
                {
                    return Err(invalid_at(
                        "responses-event",
                        Some(kind),
                        "output_index",
                        Some(observed_bytes),
                        Some(MAX_EVENT),
                    ));
                }
                events.push(ProviderStreamEvent::ToolCallStarted {
                    index: idx,
                    call_id: Some(id),
                    name: Some(ProviderToolName::new(name).map_err(|_| {
                        invalid_at(
                            "responses-event",
                            Some(kind),
                            "item.name",
                            Some(observed_bytes),
                            Some(MAX_EVENT),
                        )
                    })?),
                });
            }
            "response.function_call_arguments.delta" => {
                self.tool_started = true;
                let idx = index()?;
                let call = self.calls.get_mut(&idx).ok_or_else(|| {
                    invalid_at(
                        "responses-event",
                        Some(kind),
                        "output_index",
                        Some(observed_bytes),
                        Some(MAX_EVENT),
                    )
                })?;
                let delta = value["delta"].as_str().ok_or_else(|| {
                    invalid_at(
                        "responses-event",
                        Some(kind),
                        "delta",
                        Some(observed_bytes),
                        Some(MAX_EVENT),
                    )
                })?;
                if call.done || call.args.len().saturating_add(delta.len()) > MAX_EVENT {
                    return Err(invalid_at(
                        "responses-event",
                        Some(kind),
                        "delta",
                        Some(call.args.len().saturating_add(delta.len())),
                        Some(MAX_EVENT),
                    ));
                }
                call.args.push_str(delta);
                events.push(ProviderStreamEvent::ToolCallDelta {
                    index: idx,
                    id_fragment: None,
                    name_fragment: None,
                    arguments_fragment: ContentText::new(delta).map_err(|_| {
                        invalid_at(
                            "responses-event",
                            Some(kind),
                            "delta",
                            Some(delta.len()),
                            Some(MAX_EVENT),
                        )
                    })?,
                });
            }
            "response.output_item.done" => {
                let item = &value["item"];
                match item["type"].as_str() {
                    Some("function_call") => {
                        let call = self.calls.get_mut(&index()?).ok_or_else(|| {
                            invalid_at(
                                "responses-event",
                                Some(kind),
                                "output_index",
                                Some(observed_bytes),
                                Some(MAX_EVENT),
                            )
                        })?;
                        let args = item["arguments"].as_str().ok_or_else(|| {
                            invalid_at(
                                "responses-event",
                                Some(kind),
                                "item.arguments",
                                Some(observed_bytes),
                                Some(MAX_EVENT),
                            )
                        })?;
                        if call.done
                            || item["call_id"].as_str() != Some(call.id.as_str())
                            || item["name"].as_str() != Some(&call.name)
                            || (!call.args.is_empty() && call.args != args)
                        {
                            return Err(invalid_at(
                                "responses-event",
                                Some(kind),
                                "item.identity",
                                Some(observed_bytes),
                                Some(MAX_EVENT),
                            ));
                        }
                        let arguments: Value = serde_json::from_str(args).map_err(|_| {
                            invalid_at(
                                "responses-event",
                                Some(kind),
                                "item.arguments",
                                Some(args.len()),
                                Some(MAX_EVENT),
                            )
                        })?;
                        if !arguments.is_object() {
                            return Err(invalid_at(
                                "responses-event",
                                Some(kind),
                                "item.arguments",
                                Some(args.len()),
                                Some(MAX_EVENT),
                            ));
                        }
                        call.done = true;
                        let mut extensions = ExtensionMap::default();
                        extensions
                            .insert("openai:name", json!(call.name))
                            .map_err(|_| {
                                invalid_at(
                                    "responses-event",
                                    Some(kind),
                                    "extensions",
                                    Some(observed_bytes),
                                    Some(MAX_EVENT),
                                )
                            })?;
                        events.push(ProviderStreamEvent::ToolCallCompleted(ToolCall {
                            id: call.id.clone(),
                            tool_id: self
                                .names
                                .get(&call.name)
                                .ok_or_else(|| {
                                    invalid_at(
                                        "responses-event",
                                        Some(kind),
                                        "item.name",
                                        Some(observed_bytes),
                                        Some(MAX_EVENT),
                                    )
                                })?
                                .clone(),
                            arguments,
                            extensions,
                        }));
                    }
                    Some("reasoning")
                        if item
                            .get("encrypted_content")
                            .and_then(Value::as_str)
                            .is_some()
                            && self.retention != ReasoningRetention::Disabled =>
                    {
                        events.push(ProviderStreamEvent::ContentDelta {
                            stream_id: BoundedString::new(format!("opaque-{}", index()?)).map_err(
                                |_| {
                                    invalid_at(
                                        "responses-event",
                                        Some(kind),
                                        "output_index",
                                        Some(observed_bytes),
                                        Some(MAX_EVENT),
                                    )
                                },
                            )?,
                            part: ContentPart::Reasoning(ReasoningBlock {
                                kind: ReasoningKind::OpaqueContinuation,
                                retention: self.retention,
                                text: None,
                                opaque: Some(OpaqueContent {
                                    provider_id: provider_id(),
                                    kind: "reasoning".into(),
                                    data: OpaqueProviderData::new(item.clone()).map_err(|_| {
                                        invalid_at(
                                            "responses-event",
                                            Some(kind),
                                            "item.encrypted_content",
                                            Some(observed_bytes),
                                            Some(MAX_EVENT),
                                        )
                                    })?,
                                }),
                            }),
                        })
                    }
                    Some("message") => {
                        if let Some(phase) = item.get("phase").and_then(Value::as_str) {
                            if !["commentary", "final_answer"].contains(&phase) {
                                return Err(invalid_at(
                                    "responses-event",
                                    Some(kind),
                                    "item.phase",
                                    Some(phase.len()),
                                    Some(MAX_EVENT),
                                ));
                            }
                            events.push(ProviderStreamEvent::ContentDelta {
                                stream_id: BoundedString::new("phase").expect("static"),
                                part: ContentPart::ProviderOpaque(OpaqueContent {
                                    provider_id: provider_id(),
                                    kind: "message-phase".into(),
                                    data: OpaqueProviderData::new(json!(phase)).map_err(|_| {
                                        invalid_at(
                                            "responses-event",
                                            Some(kind),
                                            "item.phase",
                                            Some(phase.len()),
                                            Some(MAX_EVENT),
                                        )
                                    })?,
                                }),
                            });
                        }
                    }
                    _ => {}
                }
            }
            "response.completed" | "response.incomplete" => {
                if self.calls.values().any(|c| !c.done) {
                    return Err(invalid_at(
                        "responses-event",
                        Some(kind),
                        "unfinished_tool_call",
                        Some(self.calls.values().filter(|call| !call.done).count()),
                        Some(self.calls.len()),
                    ));
                }
                if let Some(usage) = value["response"].get("usage").filter(|v| v.is_object()) {
                    let mut result = NormalizedUsage::unavailable(UsageMode::Cumulative);
                    let measure = |v: &Value| {
                        v.as_u64()
                            .map(UsageMeasurement::exact)
                            .unwrap_or_else(UsageMeasurement::unavailable)
                    };
                    result.input = measure(&usage["input_tokens"]);
                    result.output = measure(&usage["output_tokens"]);
                    result.total = measure(&usage["total_tokens"]);
                    result.cached_input = measure(&usage["input_tokens_details"]["cached_tokens"]);
                    result.reasoning = measure(&usage["output_tokens_details"]["reasoning_tokens"]);
                    events.push(ProviderStreamEvent::Usage(result));
                }
                let finish = if kind == "response.incomplete" {
                    match value["response"]["incomplete_details"]["reason"].as_str() {
                        Some("max_output_tokens") => FinishOutcome::OutputLimit,
                        Some("content_filter") => FinishOutcome::Safety,
                        _ => FinishOutcome::ProviderError,
                    }
                } else if self.tool_started {
                    FinishOutcome::ToolCalls
                } else {
                    FinishOutcome::Stop
                };
                events.push(self.finish(finish));
            }
            "response.failed" | "error" => {
                return Err(error(
                    "OpenAI rejected the response; check authentication, model access and account limits",
                    ErrorCategory::TransientProvider,
                    self.visible,
                ));
            }
            // Documented informational events never contain an executable call.
            "codex.response.metadata"
            | "response.metadata"
            | "response.in_progress"
            | "response.queued"
            | "response.output_item.added"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.output_text.done"
            | "response.output_text.annotation.added"
            | "response.refusal.done"
            | "response.function_call_arguments.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.done"
            | "responsesapi.websocket_timing" => {}
            _ => {
                return Err(invalid_at(
                    "responses-event",
                    Some(kind),
                    "type",
                    Some(observed_bytes),
                    Some(MAX_EVENT),
                ));
            }
        }
        Ok(events)
    }
    pub fn finish(&mut self, finish: FinishOutcome) -> ProviderStreamEvent {
        self.terminal = true;
        ProviderStreamEvent::Completed {
            finish,
            metadata: Default::default(),
        }
    }
}
