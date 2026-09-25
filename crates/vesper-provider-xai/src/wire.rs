// Keep the provider-neutral error shape at this protocol boundary.
#![allow(clippy::result_large_err)]
use crate::{XaiCatalog, error, provider_id};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use vesper_domain::*;
use vesper_provider::*;

pub(crate) const MAX_EVENT: usize = 1_048_576;
pub(crate) fn invalid() -> ProviderError {
    error(
        "xAI returned malformed or oversized Responses data",
        ErrorCategory::MalformedProtocol,
        false,
    )
}
fn unsupported() -> ProviderError {
    error(
        "xAI cannot satisfy this request control or content type",
        ErrorCategory::UnsupportedCapability,
        false,
    )
}

pub(crate) fn request(
    request: &ProviderRequest,
    default_effort: &str,
) -> Result<Value, ProviderError> {
    request_inner(request, default_effort, false)
}

pub(crate) fn request_websocket(
    request: &ProviderRequest,
    default_effort: &str,
) -> Result<Value, ProviderError> {
    request_inner(request, default_effort, true)
}

fn request_inner(
    request: &ProviderRequest,
    default_effort: &str,
    websocket: bool,
) -> Result<Value, ProviderError> {
    if request.provider_id != provider_id()
        || request.model.provider_id != provider_id()
        || XaiCatalog::find(request.model.model_id.as_str()).is_none()
    {
        return Err(unsupported());
    }
    if request.sampling.is_some() {
        return Err(unsupported());
    }
    if request
        .maximum_output_tokens
        .is_some_and(|n| n == 0 || n > 128_000)
    {
        return Err(unsupported());
    }
    let model = request.model.model_id.as_str();
    if !request.tools.is_empty() && !XaiCatalog::supports_client_tools(model) {
        return Err(unsupported());
    }
    let capabilities = XaiCatalog::find(model)
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
    if !XaiCatalog::reasoning_levels(model).contains(&effort) {
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
                    let name=call.extensions.get("xai:name").and_then(Value::as_str).or_else(||request.tools.iter().find(|t|t.id==call.tool_id).map(tool_name)).unwrap_or(call.tool_id.as_str());
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
        validate_schema(&tool.input_schema)?;
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
        tools.push(json!({"type":"function","name":name,"description":tool.description,"parameters":tool.input_schema,"strict":true}));
    }
    append_hosted_tools(&mut tools, &mut input, &request.hosted_tools)?;
    let choice = match &request.tool_choice {
        ToolChoiceIntent::Auto => json!("auto"),
        ToolChoiceIntent::None => json!("none"),
        ToolChoiceIntent::Required if !tools.is_empty() => json!("required"),
        ToolChoiceIntent::Named(id) => {
            json!({"type":"function","name":tool_name(request.tools.iter().find(|t|&t.id==id).ok_or_else(unsupported)?)})
        }
        _ => return Err(unsupported()),
    };
    let controls = request_controls(request, websocket)?;
    let mut body = json!({"model":model,"instructions":instructions.join("\n\n"),"input":input,"tools":tools,"tool_choice":choice,"parallel_tool_calls":true,"store":controls.store,"stream":true,"include":["reasoning.encrypted_content","web_search_call.action.sources","code_interpreter_call.outputs","file_search_call.results"],"reasoning":{"effort":effort,"summary":"auto"}});
    if let Some(previous) = controls.previous_response_id {
        body["previous_response_id"] = json!(previous);
    }
    if let Some(cache_key) = controls.prompt_cache_key {
        body["prompt_cache_key"] = json!(cache_key);
    }
    if websocket {
        body.as_object_mut()
            .expect("request body is an object")
            .remove("stream");
    }
    if let Some(max) = request.maximum_output_tokens {
        body["max_output_tokens"] = json!(max);
    }
    match &request.structured_output {
        StructuredOutputIntent::None => {}
        StructuredOutputIntent::JsonObject => {
            body["text"] = json!({"format":{"type":"json_object"}})
        }
        StructuredOutputIntent::JsonSchema(schema) => {
            validate_schema(schema)?;
            body["text"] = json!({"format":{"type":"json_schema","name":"vesper_output","strict":true,"schema":schema}})
        }
        _ => return Err(unsupported()),
    }
    if body.to_string().len() > 32 * MAX_EVENT {
        return Err(unsupported());
    }
    Ok(body)
}

pub(crate) fn compaction_request(
    request: &NativeCompactionRequest,
) -> Result<Value, ProviderError> {
    if request.provider_id != provider_id()
        || request.model.provider_id != provider_id()
        || XaiCatalog::find(request.model.model_id.as_str()).is_none()
    {
        return Err(unsupported());
    }
    let effort =
        XaiCatalog::default_effort(request.model.model_id.as_str()).ok_or_else(unsupported)?;
    let ordinary = ProviderRequest {
        request_id: ProviderRequestId::new("xai-native-compaction").expect("static"),
        provider_id: request.provider_id.clone(),
        model: request.model.clone(),
        endpoint_id: None,
        system_instructions: Vec::new(),
        messages: request.messages.clone(),
        tools: Vec::new(),
        hosted_tools: Vec::new(),
        tool_choice: ToolChoiceIntent::None,
        capabilities: Vec::new(),
        reasoning: None,
        structured_output: StructuredOutputIntent::None,
        sampling: None,
        maximum_output_tokens: None,
        continuation: None,
        fallback_policy: FallbackPolicy::Strict,
        provider_extensions: None,
    };
    let mut body = self::request(&ordinary, effort)?;
    let mut input = Vec::new();
    for instruction in &request.system_instructions {
        for part in &instruction.content {
            let ContentPart::Text(text) = part else {
                return Err(unsupported());
            };
            input.push(
                json!({"role":"system","content":[{"type":"input_text","text":text.as_str()}]}),
            );
        }
    }
    input.extend(
        body.get_mut("input")
            .and_then(Value::as_array_mut)
            .map(std::mem::take)
            .ok_or_else(invalid)?,
    );
    let compact = json!({"model":request.model.model_id.as_str(),"input":input});
    if compact.to_string().len() > 32 * MAX_EVENT {
        return Err(unsupported());
    }
    Ok(compact)
}

fn append_hosted_tools(
    tools: &mut Vec<Value>,
    input: &mut Vec<Value>,
    selections: &[HostedToolSelection],
) -> Result<(), ProviderError> {
    let mut ids = BTreeSet::new();
    for selection in selections {
        if !ids.insert(selection.tool_id.as_str()) {
            return Err(unsupported());
        }
        let config = selection.configuration.as_ref();
        if let Some(config) = config {
            validate_envelope(config)?;
        }
        let values = config.map(|value| &value.values);
        match selection.tool_id.as_str() {
            "web-search" | "x-search" | "code-execution" => {
                if values.is_some_and(|values| !values.is_empty()) {
                    return Err(unsupported());
                }
                let kind = match selection.tool_id.as_str() {
                    "web-search" => "web_search",
                    "x-search" => "x_search",
                    "code-execution" => "code_interpreter",
                    _ => unreachable!(),
                };
                tools.push(json!({"type":kind}));
            }
            "attachment-search" => {
                let values = values.ok_or_else(unsupported)?;
                if values
                    .iter()
                    .any(|(key, _)| !matches!(key, "xai:file-ids" | "xai:file-urls"))
                {
                    return Err(unsupported());
                }
                let mut content = Vec::new();
                append_file_references(&mut content, values, "xai:file-ids", "file_id")?;
                append_file_references(&mut content, values, "xai:file-urls", "file_url")?;
                if content.is_empty() || content.len() > 16 {
                    return Err(unsupported());
                }
                input.push(json!({"role":"user","content":content}));
            }
            "collections-search" => {
                let values = values.ok_or_else(unsupported)?;
                if values
                    .iter()
                    .any(|(key, _)| !matches!(key, "xai:collection-ids" | "xai:max-results"))
                {
                    return Err(unsupported());
                }
                let collection_ids = bounded_strings(values, "xai:collection-ids", 16, 256)?;
                if collection_ids.is_empty() {
                    return Err(unsupported());
                }
                let max = match values.get("xai:max-results") {
                    Some(value) => value.as_u64().ok_or_else(unsupported)?,
                    None => 10,
                };
                if !(1..=50).contains(&max) {
                    return Err(unsupported());
                }
                tools.push(json!({"type":"file_search","vector_store_ids":collection_ids,"max_num_results":max}));
            }
            "remote-mcp" => {
                let values = values.ok_or_else(unsupported)?;
                if values.iter().any(|(key, _)| {
                    !matches!(
                        key,
                        "xai:server-url"
                            | "xai:server-label"
                            | "xai:server-description"
                            | "xai:allowed-tools"
                    )
                }) {
                    return Err(unsupported());
                }
                let server_url = values
                    .get("xai:server-url")
                    .and_then(Value::as_str)
                    .ok_or_else(unsupported)?;
                let parsed = url::Url::parse(server_url).map_err(|_| unsupported())?;
                if parsed.scheme() != "https"
                    || server_url.len() > 2048
                    || parsed.username() != ""
                    || parsed.password().is_some()
                {
                    return Err(unsupported());
                }
                let label = values
                    .get("xai:server-label")
                    .and_then(Value::as_str)
                    .filter(|value| valid_routing_value(value, 64))
                    .ok_or_else(unsupported)?;
                let mut tool = json!({"type":"mcp","server_url":server_url,"server_label":label});
                if let Some(description) =
                    values.get("xai:server-description").and_then(Value::as_str)
                {
                    if description.len() > 1024 || description.chars().any(char::is_control) {
                        return Err(unsupported());
                    }
                    tool["server_description"] = json!(description);
                }
                if values.get("xai:allowed-tools").is_some() {
                    tool["allowed_tools"] =
                        json!(bounded_strings(values, "xai:allowed-tools", 64, 128)?);
                }
                tools.push(tool);
            }
            _ => return Err(unsupported()),
        }
    }
    Ok(())
}

fn bounded_strings(
    values: &ExtensionMap,
    key: &str,
    maximum_items: usize,
    maximum_bytes: usize,
) -> Result<Vec<String>, ProviderError> {
    let array = values
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(unsupported)?;
    if array.len() > maximum_items {
        return Err(unsupported());
    }
    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| valid_routing_value(value, maximum_bytes))
                .map(str::to_owned)
                .ok_or_else(unsupported)
        })
        .collect()
}

fn append_file_references(
    content: &mut Vec<Value>,
    values: &ExtensionMap,
    key: &str,
    wire_key: &str,
) -> Result<(), ProviderError> {
    let Some(array) = values.get(key) else {
        return Ok(());
    };
    let array = array.as_array().ok_or_else(unsupported)?;
    for item in array {
        let value = item.as_str().ok_or_else(unsupported)?;
        if wire_key == "file_url" {
            let parsed = url::Url::parse(value).map_err(|_| unsupported())?;
            if parsed.scheme() != "https" || value.len() > 8192 {
                return Err(unsupported());
            }
        } else if !valid_routing_value(value, 256) {
            return Err(unsupported());
        }
        let mut item = serde_json::Map::new();
        item.insert("type".into(), json!("input_file"));
        item.insert(wire_key.into(), json!(value));
        content.push(Value::Object(item));
    }
    Ok(())
}

#[derive(Default)]
struct RequestControls {
    previous_response_id: Option<String>,
    prompt_cache_key: Option<String>,
    store: bool,
}

fn request_controls(
    request: &ProviderRequest,
    websocket: bool,
) -> Result<RequestControls, ProviderError> {
    let mut controls = RequestControls::default();
    if let Some(continuation) = &request.continuation {
        if !continuation.may_continue() {
            return Err(unsupported());
        }
        let state = match &continuation.strategy {
            ContinuationStrategy::NativeContinuation { state }
            | ContinuationStrategy::ProviderCursor { cursor: state } => state,
            _ => return Err(unsupported()),
        };
        validate_envelope(state)?;
        let id = state
            .values
            .get("xai:previous-response-id")
            .and_then(Value::as_str)
            .filter(|value| valid_routing_value(value, 256))
            .ok_or_else(unsupported)?;
        if state
            .values
            .iter()
            .any(|(key, _)| key != "xai:previous-response-id")
        {
            return Err(unsupported());
        }
        controls.previous_response_id = Some(id.to_owned());
    }
    if let Some(extension) = &request.provider_extensions {
        validate_envelope(extension)?;
        for (key, value) in extension.values.iter() {
            match key {
                "xai:prompt-cache-key" => {
                    let value = value
                        .as_str()
                        .filter(|value| valid_routing_value(value, 128))
                        .ok_or_else(unsupported)?;
                    controls.prompt_cache_key = Some(value.to_owned());
                }
                "xai:store" => controls.store = value.as_bool().ok_or_else(unsupported)?,
                _ => return Err(unsupported()),
            }
        }
    }
    if controls.previous_response_id.is_some() && !controls.store && !websocket {
        return Err(unsupported());
    }
    Ok(controls)
}

fn validate_envelope(envelope: &VersionedExtensionEnvelope) -> Result<(), ProviderError> {
    if envelope.namespace.as_str() != "provider.xai" || envelope.version.get() != 1 {
        return Err(unsupported());
    }
    Ok(())
}

fn valid_routing_value(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn validate_schema(schema: &Value) -> Result<(), ProviderError> {
    fn visit(
        value: &Value,
        root: &Value,
        depth: usize,
        nodes: &mut usize,
        refs: &mut BTreeSet<String>,
    ) -> Result<(), ProviderError> {
        *nodes += 1;
        if depth > 32 || *nodes > 2048 || !value.is_object() {
            return Err(unsupported());
        }
        let object = value.as_object().ok_or_else(unsupported)?;
        if object.keys().any(|key| {
            matches!(
                key.as_str(),
                "not" | "if" | "then" | "else" | "contains" | "minContains" | "maxContains"
            )
        }) || object.contains_key("pattern")
        {
            return Err(unsupported());
        }
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "$schema"
                    | "$id"
                    | "$anchor"
                    | "title"
                    | "description"
                    | "default"
                    | "examples"
                    | "type"
                    | "enum"
                    | "const"
                    | "properties"
                    | "required"
                    | "additionalProperties"
                    | "items"
                    | "prefixItems"
                    | "anyOf"
                    | "oneOf"
                    | "allOf"
                    | "$ref"
                    | "$defs"
                    | "format"
                    | "minimum"
                    | "maximum"
                    | "exclusiveMinimum"
                    | "exclusiveMaximum"
                    | "minLength"
                    | "maxLength"
                    | "minItems"
                    | "maxItems"
                    | "minProperties"
                    | "maxProperties"
            ) {
                return Err(unsupported());
            }
        }
        if object
            .get("enum")
            .is_some_and(|v| v.as_array().is_none_or(Vec::is_empty))
        {
            return Err(unsupported());
        }
        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(value) = object.get(key) {
                let variants = value.as_array().ok_or_else(unsupported)?;
                if variants.is_empty() || (key == "allOf" && variants.len() != 1) {
                    return Err(unsupported());
                }
                for variant in variants {
                    visit(variant, root, depth + 1, nodes, refs)?;
                }
            }
        }
        for key in ["properties", "$defs"] {
            if let Some(values) = object.get(key) {
                let values = values.as_object().ok_or_else(unsupported)?;
                if key == "properties" && values.len() > 64 {
                    return Err(unsupported());
                }
                for child in values.values() {
                    visit(child, root, depth + 1, nodes, refs)?;
                }
            }
        }
        if let Some(items) = object.get("items") {
            visit(items, root, depth + 1, nodes, refs)?;
        }
        if let Some(items) = object.get("prefixItems") {
            for item in items.as_array().ok_or_else(unsupported)? {
                visit(item, root, depth + 1, nodes, refs)?;
            }
        }
        if object
            .get("maxLength")
            .and_then(Value::as_u64)
            .is_some_and(|v| v > 2048)
            || object
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|v| v > 256)
            || object
                .get("maxProperties")
                .and_then(Value::as_u64)
                .is_some_and(|v| v > 64)
        {
            return Err(unsupported());
        }
        if let Some(format) = object.get("format").and_then(Value::as_str)
            && ![
                "date",
                "time",
                "date-time",
                "email",
                "uuid",
                "ipv4",
                "ipv6",
                "uri",
            ]
            .contains(&format)
        {
            return Err(unsupported());
        }
        if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
            let name = reference.strip_prefix("#/$defs/").ok_or_else(unsupported)?;
            if name.is_empty() || name.contains('/') || !refs.insert(reference.to_owned()) {
                return Err(unsupported());
            }
            let target = root
                .get("$defs")
                .and_then(|defs| defs.get(name))
                .ok_or_else(unsupported)?;
            visit(target, root, depth + 1, nodes, refs)?;
            refs.remove(reference);
        }
        Ok(())
    }
    visit(schema, schema, 0, &mut 0, &mut BTreeSet::new())
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
        "citation" | "hosted-tool-result" => {}
        "compaction"
            if opaque.data.expose().get("type").and_then(Value::as_str) == Some("compaction") =>
        {
            input.push(opaque.data.expose().clone())
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
            "response.output_text.annotation.added" => {
                let annotation = value.get("annotation").ok_or_else(invalid)?;
                let url = annotation
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?;
                let parsed = url::Url::parse(url).map_err(|_| invalid())?;
                if !matches!(parsed.scheme(), "https" | "http") || url.len() > 8192 {
                    return Err(invalid());
                }
                if annotation
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| title.len() > 4096 || title.chars().any(char::is_control))
                {
                    return Err(invalid());
                }
                let start = annotation.get("start_index").and_then(Value::as_u64);
                let end = annotation.get("end_index").and_then(Value::as_u64);
                if matches!((start, end), (Some(start), Some(end)) if start > end)
                    || start.is_some() != end.is_some()
                {
                    return Err(invalid());
                }
                events.push(ProviderStreamEvent::ContentDelta {
                    stream_id: BoundedString::new(format!(
                        "citation-{}-{}",
                        index()?,
                        value
                            .get("annotation_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0)
                    ))
                    .map_err(|_| invalid())?,
                    part: ContentPart::ProviderOpaque(OpaqueContent {
                        provider_id: provider_id(),
                        kind: "citation".into(),
                        data: OpaqueProviderData::new(annotation.clone()).map_err(|_| invalid())?,
                    }),
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
                            .insert("xai:name", json!(call.name))
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
                    Some("message") => {}
                    Some(
                        "web_search_call"
                        | "x_search_call"
                        | "code_interpreter_call"
                        | "file_search_call"
                        | "mcp_call"
                        | "image_generation_call",
                    ) => events.push(ProviderStreamEvent::ContentDelta {
                        stream_id: BoundedString::new(format!("hosted-tool-{}", index()?))
                            .map_err(|_| invalid())?,
                        part: ContentPart::ProviderOpaque(OpaqueContent {
                            provider_id: provider_id(),
                            kind: "hosted-tool-result".into(),
                            data: OpaqueProviderData::new(item.clone()).map_err(|_| invalid())?,
                        }),
                    }),
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
                if let Some(citations) =
                    value["response"].get("citations").and_then(Value::as_array)
                {
                    if citations.len() > 256 {
                        return Err(invalid());
                    }
                    for (citation_index, citation) in citations.iter().enumerate() {
                        let url = citation.as_str().ok_or_else(invalid)?;
                        let parsed = url::Url::parse(url).map_err(|_| invalid())?;
                        if !matches!(parsed.scheme(), "http" | "https") || url.len() > 8192 {
                            return Err(invalid());
                        }
                        events.push(ProviderStreamEvent::ContentDelta {
                            stream_id: BoundedString::new(format!("citation-all-{citation_index}"))
                                .map_err(|_| invalid())?,
                            part: ContentPart::ProviderOpaque(OpaqueContent {
                                provider_id: provider_id(),
                                kind: "citation".into(),
                                data: OpaqueProviderData::new(
                                    json!({"type":"source_url","url":url}),
                                )
                                .map_err(|_| invalid())?,
                            }),
                        });
                    }
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
                    "xAI rejected the response; check authentication, model access and account limits",
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
