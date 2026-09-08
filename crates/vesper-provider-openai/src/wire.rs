// Keep the provider-neutral error shape at this protocol boundary.
#![allow(clippy::result_large_err)]
use crate::{OpenAiCatalog, auth::AuthenticationMode, error, provider_id};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use vesper_domain::*;
use vesper_provider::*;

pub(crate) const MAX_EVENT: usize = 1_048_576;
pub(crate) fn invalid() -> ProviderError {
    error(
        "OpenAI returned malformed or oversized Responses data",
        ErrorCategory::MalformedProtocol,
        false,
    )
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
    if request.continuation.is_some()
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
    if !OpenAiCatalog::supports_reasoning(model, effort) {
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
                    if assistant || image_count>50 || !["image/png","image/jpeg","image/webp"].contains(&image.media_type.as_str()){return Err(unsupported());}
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
        if self.terminal {
            return Err(invalid());
        }
        let mut events = vec![];
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let index = || {
            value
                .get("output_index")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(invalid)
        };
        match kind {
            "response.created" => events.push(ProviderStreamEvent::ResponseStarted {
                response_id: value["response"]["id"]
                    .as_str()
                    .map(ProviderResponseId::new)
                    .transpose()
                    .map_err(|_| invalid())?,
                metadata: Default::default(),
            }),
            "response.output_text.delta" | "response.refusal.delta" => {
                let delta = value["delta"].as_str().ok_or_else(invalid)?;
                self.output_bytes = self.output_bytes.saturating_add(delta.len() as u64);
                self.visible |= !delta.is_empty();
                events.push(ProviderStreamEvent::ContentDelta {
                    stream_id: BoundedString::new(format!("text-{}", index()?))
                        .map_err(|_| invalid())?,
                    part: ContentPart::Text(ContentText::new(delta).map_err(|_| invalid())?),
                });
            }
            "response.reasoning_summary_text.delta" => {
                let delta = value["delta"].as_str().ok_or_else(invalid)?;
                self.visible |= !delta.is_empty();
                events.push(ProviderStreamEvent::ReasoningDelta {
                    stream_id: BoundedString::new(format!("reasoning-{}", index()?))
                        .map_err(|_| invalid())?,
                    text: ContentText::new(delta).map_err(|_| invalid())?,
                    kind: ReasoningKind::Summary,
                    retention: self.retention,
                });
            }
            "response.output_item.added" if value["item"]["type"] == "function_call" => {
                self.tool_started = true;
                if self.calls.len() >= 128 {
                    return Err(invalid());
                }
                let item = &value["item"];
                let id = ToolCallId::new(item["call_id"].as_str().ok_or_else(invalid)?)
                    .map_err(|_| invalid())?;
                let name = item["name"].as_str().ok_or_else(invalid)?.to_owned();
                if !self.names.contains_key(&name) || self.calls.values().any(|c| c.id == id) {
                    return Err(invalid());
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
                    return Err(invalid());
                }
                events.push(ProviderStreamEvent::ToolCallStarted {
                    index: idx,
                    call_id: Some(id),
                    name: Some(ProviderToolName::new(name).map_err(|_| invalid())?),
                });
            }
            "response.function_call_arguments.delta" => {
                self.tool_started = true;
                let idx = index()?;
                let call = self.calls.get_mut(&idx).ok_or_else(invalid)?;
                let delta = value["delta"].as_str().ok_or_else(invalid)?;
                if call.done || call.args.len().saturating_add(delta.len()) > MAX_EVENT {
                    return Err(invalid());
                }
                call.args.push_str(delta);
                events.push(ProviderStreamEvent::ToolCallDelta {
                    index: idx,
                    id_fragment: None,
                    name_fragment: None,
                    arguments_fragment: ContentText::new(delta).map_err(|_| invalid())?,
                });
            }
            "response.output_item.done" => {
                let item = &value["item"];
                match item["type"].as_str() {
                    Some("function_call") => {
                        let call = self.calls.get_mut(&index()?).ok_or_else(invalid)?;
                        let args = item["arguments"].as_str().ok_or_else(invalid)?;
                        if call.done
                            || item["call_id"].as_str() != Some(call.id.as_str())
                            || item["name"].as_str() != Some(&call.name)
                            || (!call.args.is_empty() && call.args != args)
                        {
                            return Err(invalid());
                        }
                        let arguments: Value = serde_json::from_str(args).map_err(|_| invalid())?;
                        if !arguments.is_object() {
                            return Err(invalid());
                        }
                        call.done = true;
                        let mut extensions = ExtensionMap::default();
                        extensions
                            .insert("openai:name", json!(call.name))
                            .map_err(|_| invalid())?;
                        events.push(ProviderStreamEvent::ToolCallCompleted(ToolCall {
                            id: call.id.clone(),
                            tool_id: self.names.get(&call.name).ok_or_else(invalid)?.clone(),
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
                            stream_id: BoundedString::new(format!("opaque-{}", index()?))
                                .map_err(|_| invalid())?,
                            part: ContentPart::Reasoning(ReasoningBlock {
                                kind: ReasoningKind::OpaqueContinuation,
                                retention: self.retention,
                                text: None,
                                opaque: Some(OpaqueContent {
                                    provider_id: provider_id(),
                                    kind: "reasoning".into(),
                                    data: OpaqueProviderData::new(item.clone())
                                        .map_err(|_| invalid())?,
                                }),
                            }),
                        })
                    }
                    Some("message") => {
                        if let Some(phase) = item.get("phase").and_then(Value::as_str) {
                            if !["commentary", "final_answer"].contains(&phase) {
                                return Err(invalid());
                            }
                            events.push(ProviderStreamEvent::ContentDelta {
                                stream_id: BoundedString::new("phase").expect("static"),
                                part: ContentPart::ProviderOpaque(OpaqueContent {
                                    provider_id: provider_id(),
                                    kind: "message-phase".into(),
                                    data: OpaqueProviderData::new(json!(phase))
                                        .map_err(|_| invalid())?,
                                }),
                            });
                        }
                    }
                    _ => {}
                }
            }
            "response.completed" | "response.incomplete" => {
                if self.calls.values().any(|c| !c.done) {
                    return Err(invalid());
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
            "response.in_progress"
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
            | "response.reasoning_summary_text.done" => {}
            _ => return Err(invalid()),
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
