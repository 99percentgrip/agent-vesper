use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use vesper_domain::{
    ContentPart, ConversationMessage, FeatureRequirement, ImageDescriptor, MediaSource,
    MessageRole, ReasoningKind, SafeMessage, ToolCall, ToolChoiceIntent, ToolDefinition, ToolId,
};
use vesper_provider::{
    CapabilityResolution, ContinuationStrategy, FallbackDecision, FallbackPolicy, ProviderRequest,
    StructuredOutputIntent,
};

use crate::{
    GlmAdapterError, GlmConfig, GlmPlan, GlmReasoningMode,
    catalog::{GlmCatalog, model_output_limit},
};

/// Exact GLM wire payload plus observable pre-dispatch fallback decisions.
#[derive(Debug, Clone, PartialEq)]
pub struct SerializedGlmRequest {
    /// POST body.
    pub body: Value,
    /// Capability decisions that omitted, emulated, or fell back.
    pub fallback_decisions: Vec<FallbackDecision>,
    /// Whether reasoning history must be preserved for continuation.
    pub preserve_thinking: bool,
}

/// Serializes a neutral request into the frozen GLM chat-completions dialect.
pub fn serialize_request(
    request: &ProviderRequest,
    config: &GlmConfig,
) -> Result<SerializedGlmRequest, GlmAdapterError> {
    config.validate()?;
    if request.provider_id != crate::provider_id()
        || request.model.provider_id != crate::provider_id()
    {
        return Err(GlmAdapterError::UnsupportedRequest(
            "request belongs to another provider",
        ));
    }
    if request.model.model_id != config.model {
        return Err(GlmAdapterError::Configuration(
            "request model differs from provider session model",
        ));
    }
    if request
        .endpoint_id
        .as_ref()
        .is_some_and(|id| id != &config.endpoint.endpoint_id())
    {
        return Err(GlmAdapterError::Configuration(
            "request endpoint differs from provider session endpoint",
        ));
    }
    let descriptor =
        GlmCatalog::find(config.model.as_str()).ok_or(GlmAdapterError::UnknownModel)?;
    // Lenient capability resolution: a capability the host requires but GLM
    // cannot honor natively is downgraded to standard generation rather than
    // Lenient capability resolution: a capability mismatch never aborts the
    // turn. A required-but-unsupported capability is downgraded to standard
    // generation (recorded as a fallback decision). A control whose capability
    // intent is implied but not declared — e.g. `request.reasoning` carrying
    // reasoning through the session snapshot without an explicit
    // `provider:reasoning` intent — is tolerated rather than rejected, because
    // the GLM adapter knows its own catalog. If reasoning itself is unsupported
    // for the active model, the turn falls back to disabled thinking. A basic
    // text turn ("hello") must always succeed. Structural validation
    // (provider/model match, sampling bounds, continuation bounds) still fails.
    let reasoning_capability_unsupported = request.reasoning.is_some()
        && matches!(
            descriptor.capabilities.resolve(
                "provider:reasoning",
                FeatureRequirement::Require,
                false
            ),
            CapabilityResolution::Reject
        );
    let fallback_available = |cap: &vesper_domain::CapabilityRequest| {
        request.fallback_policy == FallbackPolicy::DeclaredOnly && cap.fallback.is_some()
    };
    let mut decisions: Vec<FallbackDecision> = Vec::with_capacity(request.capabilities.len());
    for cap in &request.capabilities {
        let resolution = descriptor.capabilities.resolve(
            cap.capability.as_str(),
            cap.requirement,
            fallback_available(cap),
        );
        if matches!(
            resolution,
            CapabilityResolution::Native | CapabilityResolution::Emulated
        ) {
            continue;
        }
        decisions.push(FallbackDecision {
            capability: cap.capability.clone(),
            resolution,
            explanation: cap
                .fallback
                .as_ref()
                .filter(|_| resolution == CapabilityResolution::Fallback)
                .map_or_else(
                    || {
                        SafeMessage::new(format!(
                            "GLM downgraded capability {} (resolved as {:?}) to standard generation",
                            cap.capability.as_str(),
                            resolution
                        ))
                        .expect("bounded fallback explanation")
                    },
                    |fallback| fallback.description.clone(),
                ),
        });
    }
    match request.validate_capabilities(&descriptor.capabilities) {
        Ok(_)
        | Err(vesper_provider::RequestValidationError::UnsupportedRequiredCapability(_))
        | Err(vesper_provider::RequestValidationError::MissingCapabilityIntent(_)) => {}
        Err(_) => {
            return Err(GlmAdapterError::UnsupportedRequest(
                "capability validation failed",
            ));
        }
    }
    if !matches!(request.structured_output, StructuredOutputIntent::None) {
        return Err(GlmAdapterError::UnsupportedRequest(
            "structured output is not confirmed by the frozen source",
        ));
    }
    validate_provider_extensions(request)?;

    let reasoning = if reasoning_capability_unsupported {
        // Reasoning capability is unavailable for the active model: fall back
        // to standard (disabled) generation rather than aborting the turn.
        GlmReasoningMode::Disabled
    } else {
        request
            .reasoning
            .as_ref()
            .and_then(|intent| intent.mode.as_ref())
            .map(|value| parse_request_reasoning(value.as_str()))
            .transpose()?
            .unwrap_or(config.reasoning)
    };
    validate_reasoning_for_model(reasoning, config.model.as_str())?;
    let preserve_thinking = reasoning != GlmReasoningMode::Disabled
        && (matches!(reasoning, GlmReasoningMode::High | GlmReasoningMode::Max)
            || config.endpoint.plan() == GlmPlan::Coding);

    let tool_names = tool_name_map(&request.tools);
    let mut messages = Vec::new();
    for instruction in &request.system_instructions {
        messages.push(message_value(
            "system",
            &instruction.content,
            preserve_thinking,
            &tool_names,
        )?);
    }
    for message in &request.messages {
        messages.push(conversation_message(
            message,
            preserve_thinking,
            &tool_names,
        )?);
    }
    if let Some(continuation) = &request.continuation {
        if !continuation.may_continue() {
            return Err(GlmAdapterError::UnsupportedRequest(
                "continuation context does not permit another request",
            ));
        }
        let continuation_message = match &continuation.strategy {
            ContinuationStrategy::ReplayWithProviderMessage { message }
                if message.namespace.as_str() == "provider.zai" && message.version.get() == 1 =>
            {
                message
                    .values
                    .get("zai:message")
                    .and_then(Value::as_str)
                    .ok_or(GlmAdapterError::UnsupportedRequest(
                        "GLM continuation message is missing",
                    ))?
            }
            ContinuationStrategy::Unsupported
            | ContinuationStrategy::ProviderCursor { .. }
            | ContinuationStrategy::NativeContinuation { .. }
            | ContinuationStrategy::ReplayWithProviderMessage { .. } => {
                return Err(GlmAdapterError::UnsupportedRequest(
                    "continuation strategy is not supported by GLM",
                ));
            }
        };
        if continuation_message != crate::continuation_message() {
            return Err(GlmAdapterError::UnsupportedRequest(
                "continuation message does not match frozen GLM compatibility",
            ));
        }
        messages.push(json!({"role": "user", "content": continuation_message}));
    }

    let output_limit = model_output_limit(config.model.as_str()).unwrap_or(128_000);
    let requested = request.maximum_output_tokens.unwrap_or(output_limit);
    let maximum = requested.clamp(1, output_limit);

    let mut body = Map::new();
    body.insert("model".into(), json!(config.model.as_str()));
    body.insert("messages".into(), Value::Array(messages));
    body.insert("stream".into(), Value::Bool(true));
    body.insert("max_tokens".into(), json!(maximum));
    body.insert("stream_options".into(), json!({"include_usage": true}));
    let mut thinking = Map::new();
    thinking.insert(
        "type".into(),
        json!(if reasoning == GlmReasoningMode::Disabled {
            "disabled"
        } else {
            "enabled"
        }),
    );
    thinking.insert("clear_thinking".into(), Value::Bool(!preserve_thinking));
    body.insert("thinking".into(), Value::Object(thinking));
    match reasoning {
        GlmReasoningMode::High => {
            body.insert("reasoning_effort".into(), json!("high"));
        }
        GlmReasoningMode::Max => {
            body.insert("reasoning_effort".into(), json!("max"));
        }
        GlmReasoningMode::Disabled | GlmReasoningMode::Enabled => {}
    }

    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(tool_definition)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        );
        body.insert("tool_stream".into(), Value::Bool(true));
        if !matches!(request.tool_choice, ToolChoiceIntent::Auto) {
            body.insert(
                "tool_choice".into(),
                tool_choice_value(&request.tool_choice, &request.tools)?,
            );
        }
    } else if !matches!(request.tool_choice, ToolChoiceIntent::None) {
        return Err(GlmAdapterError::UnsupportedRequest(
            "tool choice requires tool definitions",
        ));
    }

    let profile_controls = config.generation_profile.controls();
    let sampling = request.sampling.as_ref();
    let temperature = sampling
        .and_then(|value| value.temperature)
        .or(profile_controls.0);
    let top_p = sampling
        .and_then(|value| value.top_p)
        .or(profile_controls.1);
    if sampling.is_some_and(|value| value.seed.is_some() || !value.extensions.is_empty()) {
        return Err(GlmAdapterError::UnsupportedRequest(
            "GLM does not support requested sampling extensions",
        ));
    }
    if let Some(value) = temperature {
        if !value.is_finite() {
            return Err(GlmAdapterError::UnsupportedRequest(
                "temperature must be finite",
            ));
        }
        body.insert("temperature".into(), json!(value));
    }
    if let Some(value) = top_p {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(GlmAdapterError::UnsupportedRequest(
                "top_p must be within zero and one",
            ));
        }
        body.insert("top_p".into(), json!(value));
    }

    Ok(SerializedGlmRequest {
        body: Value::Object(body),
        fallback_decisions: decisions
            .into_iter()
            .filter(|decision| {
                !matches!(
                    decision.resolution,
                    CapabilityResolution::Native | CapabilityResolution::Emulated
                )
            })
            .collect(),
        preserve_thinking,
    })
}

fn validate_provider_extensions(request: &ProviderRequest) -> Result<(), GlmAdapterError> {
    let Some(envelope) = &request.provider_extensions else {
        return Ok(());
    };
    if envelope.namespace.as_str() != "provider.zai" || envelope.version.get() != 1 {
        return Err(GlmAdapterError::UnsupportedRequest(
            "provider extension namespace or version is unsupported",
        ));
    }
    let allowed = BTreeSet::from(["zai:compatibility-tag"]);
    if envelope
        .values
        .iter()
        .any(|(key, _)| !allowed.contains(key))
    {
        return Err(GlmAdapterError::UnsupportedRequest(
            "provider extension field is not allowlisted",
        ));
    }
    Ok(())
}

fn parse_request_reasoning(value: &str) -> Result<GlmReasoningMode, GlmAdapterError> {
    match value {
        "disabled" => Ok(GlmReasoningMode::Disabled),
        "enabled" | "standard" => Ok(GlmReasoningMode::Enabled),
        "high" => Ok(GlmReasoningMode::High),
        "max" => Ok(GlmReasoningMode::Max),
        _ => Err(GlmAdapterError::UnsupportedRequest(
            "reasoning mode is not supported by GLM",
        )),
    }
}

fn validate_reasoning_for_model(
    reasoning: GlmReasoningMode,
    model: &str,
) -> Result<(), GlmAdapterError> {
    if matches!(reasoning, GlmReasoningMode::High | GlmReasoningMode::Max) && model != "glm-5.2" {
        Err(GlmAdapterError::UnsupportedRequest(
            "high and max reasoning require glm-5.2",
        ))
    } else {
        Ok(())
    }
}

fn conversation_message(
    message: &ConversationMessage,
    preserve_thinking: bool,
    tool_names: &BTreeMap<ToolId, String>,
) -> Result<Value, GlmAdapterError> {
    let role = match &message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
        MessageRole::ProviderOpaque(_) => {
            return Err(GlmAdapterError::UnsupportedRequest(
                "provider-opaque role is unsupported by GLM",
            ));
        }
    };
    message_value(role, &message.content, preserve_thinking, tool_names)
}

fn message_value(
    role: &str,
    parts: &[ContentPart],
    preserve_thinking: bool,
    tool_names: &BTreeMap<ToolId, String>,
) -> Result<Value, GlmAdapterError> {
    let mut text = Vec::new();
    let mut images = Vec::new();
    let mut reasoning = Vec::new();
    let mut calls = Vec::new();
    let mut results = Vec::new();
    for part in parts {
        match part {
            ContentPart::Text(value) => text.push(value.as_str()),
            ContentPart::Image(value) => images.push(image_value(value)?),
            ContentPart::Reasoning(block)
                if block.kind == ReasoningKind::ProviderVisible && block.text.is_some() =>
            {
                reasoning.push(block.text.as_ref().expect("guarded").as_str());
            }
            ContentPart::Reasoning(block) if block.kind == ReasoningKind::OpaqueContinuation => {
                return Err(GlmAdapterError::UnsupportedRequest(
                    "GLM does not accept opaque reasoning records",
                ));
            }
            ContentPart::Reasoning(_) => {}
            ContentPart::ToolCall(call) => calls.push(tool_call_value(call, tool_names)?),
            ContentPart::ToolResult(result) => results.push(result),
            ContentPart::Audio(_) => {
                return Err(GlmAdapterError::UnsupportedRequest(
                    "GLM adapter does not support audio",
                ));
            }
            ContentPart::EmbeddedContext(_) => {
                return Err(GlmAdapterError::UnsupportedRequest(
                    "embedded context must be resolved before provider dispatch",
                ));
            }
            ContentPart::ProviderOpaque(_) => {
                return Err(GlmAdapterError::UnsupportedRequest(
                    "provider-opaque message content is not allowlisted",
                ));
            }
        }
    }
    if !results.is_empty() {
        if results.len() != 1 || !text.is_empty() || !images.is_empty() || !calls.is_empty() {
            return Err(GlmAdapterError::UnsupportedRequest(
                "GLM tool-result messages must contain exactly one result",
            ));
        }
        let result = results[0];
        return Ok(json!({
            "role": "tool",
            "tool_call_id": result.call_id.as_str(),
            "content": value_as_text(&result.output),
        }));
    }
    let mut value = Map::new();
    value.insert("role".into(), json!(role));
    if images.is_empty() {
        value.insert("content".into(), json!(text.concat()));
    } else {
        let mut content = text
            .into_iter()
            .filter(|value| !value.is_empty())
            .map(|value| json!({"type": "text", "text": value}))
            .collect::<Vec<_>>();
        content.extend(images);
        value.insert("content".into(), Value::Array(content));
    }
    if preserve_thinking && !reasoning.is_empty() {
        value.insert("reasoning_content".into(), json!(reasoning.concat()));
    }
    if !calls.is_empty() {
        value.insert("tool_calls".into(), Value::Array(calls));
    }
    Ok(Value::Object(value))
}

fn image_value(image: &ImageDescriptor) -> Result<Value, GlmAdapterError> {
    let reference = match &image.source {
        MediaSource::Reference { reference } => reference,
        MediaSource::InlineDescriptor(_) => {
            return Err(GlmAdapterError::UnsupportedRequest(
                "inline image descriptors require an external byte carrier",
            ));
        }
    };
    if reference.len() > 4 * 1024 * 1024 {
        return Err(GlmAdapterError::Limit("image reference"));
    }
    Ok(json!({"type": "image_url", "image_url": {"url": reference}}))
}

fn tool_name_map(tools: &[ToolDefinition]) -> BTreeMap<ToolId, String> {
    tools
        .iter()
        .map(|tool| {
            (
                tool.id.clone(),
                tool.provider_name
                    .as_ref()
                    .map_or_else(|| tool.harness_name.as_str(), |name| name.as_str())
                    .to_owned(),
            )
        })
        .collect()
}

fn tool_call_value(
    call: &ToolCall,
    tool_names: &BTreeMap<ToolId, String>,
) -> Result<Value, GlmAdapterError> {
    let name = tool_names
        .get(&call.tool_id)
        .ok_or(GlmAdapterError::UnsupportedRequest(
            "tool call references an undefined tool",
        ))?;
    let arguments = serde_json::to_string(&call.arguments)
        .map_err(|_| GlmAdapterError::UnsupportedRequest("tool arguments cannot serialize"))?;
    if arguments.len() > crate::sse::MAX_TOOL_ARGUMENT_BYTES {
        return Err(GlmAdapterError::Limit("tool arguments"));
    }
    Ok(json!({
        "id": call.id.as_str(),
        "type": "function",
        "function": {"name": name, "arguments": arguments},
    }))
}

fn tool_definition(tool: &ToolDefinition) -> Result<Value, GlmAdapterError> {
    let name = tool
        .provider_name
        .as_ref()
        .map_or_else(|| tool.harness_name.as_str(), |name| name.as_str());
    if name.len() > crate::sse::MAX_TOOL_NAME_BYTES {
        return Err(GlmAdapterError::Limit("tool name"));
    }
    if serde_json::to_vec(&tool.input_schema)
        .map_err(|_| GlmAdapterError::UnsupportedRequest("tool schema cannot serialize"))?
        .len()
        > 256 * 1024
    {
        return Err(GlmAdapterError::Limit("tool schema"));
    }
    let mut function = Map::new();
    function.insert("name".into(), json!(name));
    if !tool.description.is_empty() {
        function.insert("description".into(), json!(tool.description));
    }
    function.insert("parameters".into(), tool.input_schema.clone());
    Ok(json!({"type": "function", "function": Value::Object(function)}))
}

fn tool_choice_value(
    choice: &ToolChoiceIntent,
    tools: &[ToolDefinition],
) -> Result<Value, GlmAdapterError> {
    match choice {
        ToolChoiceIntent::Auto => Ok(json!("auto")),
        ToolChoiceIntent::None => Ok(json!("none")),
        ToolChoiceIntent::Required => Ok(json!("required")),
        ToolChoiceIntent::Named(id) => {
            let tool = tools.iter().find(|tool| &tool.id == id).ok_or(
                GlmAdapterError::UnsupportedRequest("named tool choice is undefined"),
            )?;
            let name = tool
                .provider_name
                .as_ref()
                .map_or_else(|| tool.harness_name.as_str(), |name| name.as_str());
            Ok(json!({"type": "function", "function": {"name": name}}))
        }
        ToolChoiceIntent::ProviderExtension(_) => Err(GlmAdapterError::UnsupportedRequest(
            "provider tool-choice extension is not allowlisted",
        )),
    }
}

fn value_as_text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), ToOwned::to_owned)
}

/// Builds one source-compatible bounded auxiliary request body.
pub fn serialize_auxiliary_request(
    request: &ProviderRequest,
    config: &GlmConfig,
) -> Result<Value, GlmAdapterError> {
    if request.continuation.is_some() {
        return Err(GlmAdapterError::UnsupportedRequest(
            "auxiliary requests cannot continue",
        ));
    }
    let serialized = serialize_request(request, config)?;
    let mut body = serialized
        .body
        .as_object()
        .cloned()
        .ok_or(GlmAdapterError::MalformedProtocol)?;
    body.insert("stream".into(), Value::Bool(false));
    body.remove("stream_options");
    body.remove("tools");
    body.remove("tool_stream");
    body.remove("tool_choice");
    body.insert("thinking".into(), json!({"type": "disabled"}));
    let maximum = body
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(1_200)
        .clamp(1, 4_096);
    body.insert("max_tokens".into(), json!(maximum));
    Ok(Value::Object(body))
}

#[cfg(test)]
mod tests {
    use vesper_domain::{
        CapabilityId, CapabilityRequest, FeatureRequirement, HarnessToolName, MessageId, ModelId,
        ProviderId, ProviderRequestId, QualifiedModelId, ReasoningRetention, ToolDefinition,
        ToolExecutionClass, ToolId,
    };
    use vesper_provider::{
        FallbackPolicy, ReasoningIntent, SamplingIntent, StructuredOutputIntent,
    };

    use super::*;
    use crate::GlmGenerationProfile;

    fn base_request() -> ProviderRequest {
        ProviderRequest {
            request_id: ProviderRequestId::new("request-1").unwrap(),
            provider_id: ProviderId::new("zai").unwrap(),
            model: QualifiedModelId {
                provider_id: ProviderId::new("zai").unwrap(),
                model_id: ModelId::new("glm-5.2").unwrap(),
            },
            endpoint_id: Some(vesper_domain::EndpointId::new("zai-coding").unwrap()),
            system_instructions: Vec::new(),
            messages: vec![ConversationMessage {
                id: MessageId::new("message-1").unwrap(),
                role: MessageRole::User,
                content: vec![ContentPart::Text(
                    vesper_domain::ContentText::new("hello").unwrap(),
                )],
                extensions: Default::default(),
            }],
            tools: Vec::new(),
            tool_choice: ToolChoiceIntent::None,
            capabilities: vec![
                CapabilityRequest {
                    capability: CapabilityId::new("provider:reasoning").unwrap(),
                    requirement: FeatureRequirement::Require,
                    fallback: None,
                },
                CapabilityRequest {
                    capability: CapabilityId::new("provider:streamed-reasoning").unwrap(),
                    requirement: FeatureRequirement::Require,
                    fallback: None,
                },
                CapabilityRequest {
                    capability: CapabilityId::new("provider:limits").unwrap(),
                    requirement: FeatureRequirement::Require,
                    fallback: None,
                },
                CapabilityRequest {
                    capability: CapabilityId::new("provider:sampling").unwrap(),
                    requirement: FeatureRequirement::Require,
                    fallback: None,
                },
            ],
            reasoning: Some(ReasoningIntent {
                mode: Some(vesper_domain::BoundedString::new("high").unwrap()),
                stream_visible: true,
                retention: ReasoningRetention::Persist,
            }),
            structured_output: StructuredOutputIntent::None,
            sampling: Some(SamplingIntent {
                temperature: Some(0.7),
                top_p: None,
                seed: None,
                extensions: Default::default(),
            }),
            maximum_output_tokens: Some(1_024),
            continuation: None,
            fallback_policy: FallbackPolicy::Strict,
            provider_extensions: None,
        }
    }

    #[test]
    fn exact_high_reasoning_request_is_source_compatible() {
        let request = serialize_request(&base_request(), &GlmConfig::default()).unwrap();
        assert_eq!(
            request.body,
            json!({
                "model": "glm-5.2",
                "messages": [{"role": "user", "content": "hello"}],
                "stream": true,
                "max_tokens": 1024,
                "stream_options": {"include_usage": true},
                "thinking": {"type": "enabled", "clear_thinking": false},
                "reasoning_effort": "high",
                "temperature": 0.7
            })
        );
        assert!(request.preserve_thinking);
    }

    #[test]
    fn max_reasoning_request_emits_max_reasoning_effort() {
        // ADR 0009 / Tier A end-to-end (serializer half): a request carrying
        // the session-scoped override `reasoning.mode = "max"` must serialize
        // to the GLM wire `reasoning_effort: "max"`. The runtime half
        // (`session_reasoning_override_threads_into_the_provider_request`) plus
        // the TUI half (`thinking_command_sets_a_pending_reasoning_update`)
        // complete the `/thinking max` → wire chain.
        let mut request = base_request();
        request.reasoning.as_mut().expect("base has reasoning").mode =
            Some(vesper_domain::BoundedString::new("max").unwrap());
        let serialized = serialize_request(&request, &GlmConfig::default()).unwrap();
        assert_eq!(serialized.body["reasoning_effort"], json!("max"));
        assert_eq!(
            serialized.body["thinking"]["type"],
            json!("enabled"),
            "max reasoning keeps thinking enabled"
        );
    }

    #[test]
    fn auxiliary_is_bounded_and_disables_thinking_and_streaming() {
        let body = serialize_auxiliary_request(&base_request(), &GlmConfig::default()).unwrap();
        assert_eq!(body["stream"], false);
        assert_eq!(body["thinking"], json!({"type": "disabled"}));
        assert_eq!(body["max_tokens"], 1_024);
        assert!(body.get("stream_options").is_none());
    }

    #[test]
    fn generation_profiles_change_only_one_sampling_control() {
        assert_eq!(GlmGenerationProfile::Balanced.controls(), (None, None));
        assert_eq!(GlmGenerationProfile::Precise.controls(), (Some(0.7), None));
        assert_eq!(
            GlmGenerationProfile::Exploratory.controls(),
            (None, Some(0.98))
        );
    }

    #[test]
    fn exact_tool_and_named_choice_serialization_preserves_schema() {
        let mut request = base_request();
        request.capabilities.extend([
            CapabilityRequest {
                capability: CapabilityId::new("provider:tools").unwrap(),
                requirement: FeatureRequirement::Require,
                fallback: None,
            },
            CapabilityRequest {
                capability: CapabilityId::new("provider:tool-choice").unwrap(),
                requirement: FeatureRequirement::Require,
                fallback: None,
            },
        ]);
        let tool = ToolDefinition {
            id: ToolId::new("read-file").unwrap(),
            harness_name: HarnessToolName::new("read_file").unwrap(),
            provider_name: None,
            description: "Read one file".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
            execution_class: ToolExecutionClass::ReadOnly,
            extensions: Default::default(),
        };
        request.tool_choice = ToolChoiceIntent::Named(tool.id.clone());
        request.tools.push(tool);

        let body = serialize_request(&request, &GlmConfig::default())
            .unwrap()
            .body;
        assert_eq!(body["tool_stream"], true);
        assert_eq!(
            body["tool_choice"],
            json!({"type":"function","function":{"name":"read_file"}})
        );
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["required"],
            json!(["path"])
        );
    }

    #[test]
    fn continuation_is_adapter_owned_and_strictly_namespaced() {
        let mut request = base_request();
        request.capabilities.push(CapabilityRequest {
            capability: CapabilityId::new("provider:continuation").unwrap(),
            requirement: FeatureRequirement::Require,
            fallback: None,
        });
        let mut metadata = vesper_domain::ExtensionMap::default();
        metadata
            .insert(
                "zai:message",
                json!("Continue exactly where you left off. Do not repeat or summarize."),
            )
            .unwrap();
        let message = vesper_domain::VersionedExtensionEnvelope {
            namespace: vesper_domain::ExtensionNamespace::new("provider.zai").unwrap(),
            version: vesper_domain::SchemaVersion::new(1).unwrap(),
            values: metadata,
        };
        request.continuation = Some(vesper_provider::ContinuationContext {
            strategy: vesper_provider::ContinuationStrategy::ReplayWithProviderMessage { message },
            provider_maximum: Some(20),
            harness_maximum: 20,
            visible_count: 1,
            reason: vesper_provider::ContinuationReason::OutputLimit,
            metadata: Default::default(),
        });
        let body = serialize_request(&request, &GlmConfig::default())
            .unwrap()
            .body;
        assert_eq!(
            body["messages"].as_array().unwrap().last().unwrap(),
            &json!({"role":"user","content":"Continue exactly where you left off. Do not repeat or summarize."})
        );

        if let vesper_provider::ContinuationStrategy::ReplayWithProviderMessage { message } =
            &mut request.continuation.as_mut().unwrap().strategy
        {
            message.namespace = vesper_domain::ExtensionNamespace::new("provider.other").unwrap();
        }
        assert!(serialize_request(&request, &GlmConfig::default()).is_err());
    }

    #[test]
    fn agent_loop_first_turn_serializes_without_capability_error() {
        // Mirrors vesper-agent agent_loop::build_request for Code mode: a
        // non-empty tool surface advertised with provider:tools and
        // provider:tool-choice as FeatureRequirement::Require, reasoning None
        // (so the GLM serializer falls back to config.reasoning = Enabled).
        let mut request = base_request();
        request.reasoning = None;
        request.sampling = None;
        request.maximum_output_tokens = None;
        request.capabilities = vec![
            CapabilityRequest {
                capability: CapabilityId::new("provider:tools").unwrap(),
                requirement: FeatureRequirement::Require,
                fallback: None,
            },
            CapabilityRequest {
                capability: CapabilityId::new("provider:tool-choice").unwrap(),
                requirement: FeatureRequirement::Require,
                fallback: None,
            },
        ];
        request.tools.push(ToolDefinition {
            id: ToolId::new("read-file").unwrap(),
            harness_name: HarnessToolName::new("read_file").unwrap(),
            provider_name: None,
            description: "Read one file".into(),
            input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            execution_class: ToolExecutionClass::ReadOnly,
            extensions: Default::default(),
        });
        request.tool_choice = ToolChoiceIntent::Auto;
        let result = serialize_request(&request, &GlmConfig::default());
        assert!(
            result.is_ok(),
            "agent-loop first turn must serialize: {result:?}"
        );
    }

    #[test]
    fn unsupported_required_capability_falls_back_instead_of_crashing() {
        // GLM catalog declares provider:audio as Unsupported. Requesting it
        // with FeatureRequirement::Require previously threw UnsupportedCapability
        // and aborted the turn; the adapter now downgrades it to standard
        // generation and records a fallback decision so a basic turn still
        // succeeds. (Directive: fall back, do not throw a hard error.)
        let mut request = base_request();
        request.capabilities.push(CapabilityRequest {
            capability: CapabilityId::new("provider:audio").unwrap(),
            requirement: FeatureRequirement::Require,
            fallback: None,
        });
        let serialized = serialize_request(&request, &GlmConfig::default())
            .expect("unsupported required capability must fall back, not crash");
        assert!(
            serialized.fallback_decisions.iter().any(|decision| {
                decision.capability.as_str() == "provider:audio"
                    && decision.resolution == CapabilityResolution::Reject
            }),
            "the unsupported audio capability must be recorded as a Reject fallback decision: {:?}",
            serialized.fallback_decisions
        );
    }

    #[test]
    fn reasoning_without_declared_capability_intent_does_not_crash() {
        // Regression for the real first-turn crash: a host threads
        // `reasoning = Some(enabled)` through the session snapshot WITHOUT an
        // explicit `provider:reasoning` capability intent. validate_capabilities
        // raises MissingCapabilityIntent, which previously surfaced as
        // UnsupportedCapability and aborted the turn. The adapter now tolerates
        // it and serializes a standard thinking-enabled request.
        let mut request = base_request();
        request.capabilities = Vec::new();
        request.reasoning = Some(ReasoningIntent {
            mode: Some(vesper_domain::BoundedString::new("enabled").unwrap()),
            stream_visible: true,
            retention: ReasoningRetention::Persist,
        });
        let serialized = serialize_request(&request, &GlmConfig::default())
            .expect("reasoning without a declared capability intent must not crash");
        assert_eq!(
            serialized.body["thinking"]["type"],
            json!("enabled"),
            "reasoning stays enabled when GLM supports it natively"
        );
    }
}
