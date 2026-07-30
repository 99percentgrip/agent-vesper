use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use vesper_domain::{
    BoundedString, ContentPart, ContentText, ExtensionMap, FinishOutcome, NormalizedUsage,
    ProviderRequestId, ProviderToolName, ReasoningKind, ReasoningRetention, ToolCall, ToolCallId,
    ToolId, UsageMeasurement, UsageMode,
};
use vesper_provider::ProviderStreamEvent;

use crate::{
    GlmAdapterError,
    sse::{MAX_PROVIDER_METADATA_BYTES, MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_NAME_BYTES},
};

#[derive(Debug, Default)]
struct ToolAccumulator {
    id: String,
    name: String,
    arguments: String,
    announced: bool,
}

/// Result of one HTTP stream attempt before retry/continuation policy.
#[derive(Debug, Default)]
pub(crate) struct AttemptState {
    pub(crate) content: String,
    pub(crate) reasoning: String,
    pub(crate) finish_reason: Option<String>,
    pub(crate) usage: Option<NormalizedUsage>,
    pub(crate) visible: bool,
    pub(crate) done_seen: bool,
    tool_calls: BTreeMap<u32, ToolAccumulator>,
}

impl AttemptState {
    /// Parses one GLM `data:` JSON payload. Malformed JSON is ignored to match
    /// the frozen parser; structurally dangerous/bound-violating data errors.
    pub(crate) fn accept_data(
        &mut self,
        data: &str,
        request_id: &ProviderRequestId,
        tool_ids: &BTreeMap<String, ToolId>,
    ) -> Result<Vec<ProviderStreamEvent>, GlmAdapterError> {
        let value: Value = match serde_json::from_str(data) {
            Ok(value) => value,
            Err(_) => return Ok(Vec::new()),
        };
        let object = value
            .as_object()
            .ok_or(GlmAdapterError::MalformedProtocol)?;
        let mut events = Vec::new();

        if let Some(usage) = object.get("usage").filter(|value| !value.is_null()) {
            self.usage = Some(normalize_usage(usage)?);
        }
        let Some(choices) = object.get("choices") else {
            return Ok(events);
        };
        let choices = choices
            .as_array()
            .ok_or(GlmAdapterError::MalformedProtocol)?;
        let Some(choice) = choices.first() else {
            return Ok(events);
        };
        let choice = choice
            .as_object()
            .ok_or(GlmAdapterError::MalformedProtocol)?;
        let delta = choice
            .get("delta")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        if let Some(reasoning) = delta
            .get("reasoning_content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let text = ContentText::new(reasoning)
                .map_err(|_| GlmAdapterError::Limit("reasoning delta"))?;
            self.reasoning.push_str(reasoning);
            self.visible = true;
            events.push(ProviderStreamEvent::ReasoningDelta {
                stream_id: BoundedString::new("reasoning").expect("static stream ID"),
                text,
                kind: ReasoningKind::ProviderVisible,
                retention: ReasoningRetention::SessionOnly,
            });
        }
        if let Some(content) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let text =
                ContentText::new(content).map_err(|_| GlmAdapterError::Limit("content delta"))?;
            self.content.push_str(content);
            self.visible = true;
            events.push(ProviderStreamEvent::ContentDelta {
                stream_id: BoundedString::new("content").expect("static stream ID"),
                part: ContentPart::Text(text),
            });
        }

        if let Some(calls) = delta.get("tool_calls") {
            let calls = calls.as_array().ok_or(GlmAdapterError::MalformedProtocol)?;
            for call in calls {
                let call = call.as_object().ok_or(GlmAdapterError::MalformedProtocol)?;
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
                let index =
                    u32::try_from(index).map_err(|_| GlmAdapterError::Limit("tool-call index"))?;
                let id_fragment = call.get("id").and_then(Value::as_str).unwrap_or_default();
                let function = call.get("function").and_then(Value::as_object);
                let name_fragment = function
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let arguments_fragment = function
                    .and_then(|value| value.get("arguments"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let accumulator = self.tool_calls.entry(index).or_default();
                append_bounded(&mut accumulator.id, id_fragment, 256, "tool-call ID")?;
                append_bounded(
                    &mut accumulator.name,
                    name_fragment,
                    MAX_TOOL_NAME_BYTES,
                    "tool name",
                )?;
                append_bounded(
                    &mut accumulator.arguments,
                    arguments_fragment,
                    MAX_TOOL_ARGUMENT_BYTES,
                    "tool arguments",
                )?;
                let resolved_id = resolved_call_id(request_id, index, &accumulator.id)?;
                if !accumulator.announced && !accumulator.name.is_empty() {
                    accumulator.announced = true;
                    self.visible = true;
                    events.push(ProviderStreamEvent::ToolCallStarted {
                        index,
                        call_id: Some(resolved_id.clone()),
                        name: Some(
                            ProviderToolName::new(accumulator.name.clone())
                                .map_err(|_| GlmAdapterError::Limit("tool name"))?,
                        ),
                    });
                }
                if !id_fragment.is_empty()
                    || !name_fragment.is_empty()
                    || !arguments_fragment.is_empty()
                {
                    self.visible = true;
                    events.push(ProviderStreamEvent::ToolCallDelta {
                        index,
                        id_fragment: (!id_fragment.is_empty())
                            .then(|| BoundedString::new(id_fragment))
                            .transpose()
                            .map_err(|_| GlmAdapterError::Limit("tool-call ID fragment"))?,
                        name_fragment: (!name_fragment.is_empty())
                            .then(|| BoundedString::new(name_fragment))
                            .transpose()
                            .map_err(|_| GlmAdapterError::Limit("tool name fragment"))?,
                        arguments_fragment: ContentText::new(arguments_fragment)
                            .map_err(|_| GlmAdapterError::Limit("tool arguments fragment"))?,
                    });
                }
            }
        }
        if let Some(finish) = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            if finish.len() > 256 || finish.chars().any(char::is_control) {
                return Err(GlmAdapterError::Limit("finish reason"));
            }
            self.finish_reason = Some(finish.to_owned());
        }
        let _ = tool_ids;
        Ok(events)
    }

    pub(crate) fn mark_done(&mut self) {
        self.done_seen = true;
    }

    pub(crate) fn terminal_seen(&self) -> bool {
        self.done_seen || self.finish_reason.is_some()
    }

    pub(crate) fn complete_tool_events(
        &mut self,
        request_id: &ProviderRequestId,
        tool_ids: &BTreeMap<String, ToolId>,
    ) -> Result<Vec<ProviderStreamEvent>, GlmAdapterError> {
        let mut events = Vec::new();
        for (index, accumulator) in &self.tool_calls {
            if accumulator.name.is_empty() {
                continue;
            }
            let id = resolved_call_id(request_id, *index, &accumulator.id)?;
            let arguments = if accumulator.arguments.is_empty() {
                json!({})
            } else {
                serde_json::from_str(&accumulator.arguments)
                    .unwrap_or_else(|_| json!({"_raw": accumulator.arguments}))
            };
            let tool_id = tool_ids.get(&accumulator.name).cloned().unwrap_or_else(|| {
                ToolId::new(accumulator.name.clone()).expect("bounded tool name is valid ID")
            });
            let mut extensions = ExtensionMap::default();
            extensions
                .insert("zai:stream-index", json!(index))
                .expect("bounded stream index metadata");
            events.push(ProviderStreamEvent::ToolCallCompleted(ToolCall {
                id,
                tool_id,
                arguments,
                extensions,
            }));
        }
        Ok(events)
    }

    pub(crate) fn has_tool_calls(&self) -> bool {
        self.tool_calls
            .values()
            .any(|accumulator| !accumulator.name.is_empty())
    }
}

fn append_bounded(
    target: &mut String,
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), GlmAdapterError> {
    if target.len().saturating_add(value.len()) > maximum {
        return Err(GlmAdapterError::Limit(field));
    }
    target.push_str(value);
    Ok(())
}

fn resolved_call_id(
    request_id: &ProviderRequestId,
    index: u32,
    provider_id: &str,
) -> Result<ToolCallId, GlmAdapterError> {
    if provider_id.is_empty() {
        let request = request_id.as_str().chars().take(220).collect::<String>();
        ToolCallId::new(format!("call_{request}_{index}"))
            .map_err(|_| GlmAdapterError::Limit("generated tool-call ID"))
    } else {
        ToolCallId::new(provider_id).map_err(|_| GlmAdapterError::Limit("provider tool-call ID"))
    }
}

pub(crate) fn tool_id_map(tools: &[vesper_domain::ToolDefinition]) -> BTreeMap<String, ToolId> {
    tools
        .iter()
        .map(|tool| {
            (
                tool.provider_name
                    .as_ref()
                    .map_or_else(|| tool.harness_name.as_str(), |name| name.as_str())
                    .to_owned(),
                tool.id.clone(),
            )
        })
        .collect()
}

pub(crate) fn normalize_usage(value: &Value) -> Result<NormalizedUsage, GlmAdapterError> {
    let object = value
        .as_object()
        .ok_or(GlmAdapterError::MalformedProtocol)?;
    let details = object
        .get("prompt_tokens_details")
        .and_then(Value::as_object);
    let mut usage = NormalizedUsage::unavailable(UsageMode::Delta);
    usage.input = exact_optional(object.get("prompt_tokens").or(object.get("input_tokens")))?;
    usage.output = exact_optional(
        object
            .get("completion_tokens")
            .or(object.get("output_tokens")),
    )?;
    usage.total = exact_optional(object.get("total_tokens"))?;
    usage.cached_input = exact_optional(details.and_then(|details| details.get("cached_tokens")))?;
    let reasoning = object
        .get("completion_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("reasoning_tokens"));
    usage.reasoning = exact_optional(reasoning)?;
    let mut raw = serde_json::Map::new();
    let known: BTreeSet<&str> = [
        "prompt_tokens",
        "input_tokens",
        "completion_tokens",
        "output_tokens",
        "total_tokens",
        "prompt_tokens_details",
        "completion_tokens_details",
    ]
    .into_iter()
    .collect();
    for (key, value) in object {
        if !known.contains(key.as_str()) {
            raw.insert(key.clone(), value.clone());
        }
    }
    if !raw.is_empty() {
        let encoded = serde_json::to_vec(&raw).map_err(|_| GlmAdapterError::MalformedProtocol)?;
        if encoded.len() > MAX_PROVIDER_METADATA_BYTES {
            return Err(GlmAdapterError::Limit("provider usage metadata"));
        }
        usage
            .provider_metadata
            .insert("zai:raw-usage", Value::Object(raw))
            .map_err(|_| GlmAdapterError::Limit("provider usage metadata"))?;
    }
    Ok(usage)
}

fn exact_optional(value: Option<&Value>) -> Result<UsageMeasurement, GlmAdapterError> {
    match value {
        None | Some(Value::Null) => Ok(UsageMeasurement::unavailable()),
        Some(value) => value
            .as_u64()
            .map(UsageMeasurement::exact)
            .ok_or(GlmAdapterError::MalformedProtocol),
    }
}

pub(crate) fn finish_outcome(value: Option<&str>) -> FinishOutcome {
    match value {
        Some("stop") => FinishOutcome::Stop,
        Some("tool_calls") => FinishOutcome::ToolCalls,
        Some("length") | Some("continuation_limit") => FinishOutcome::OutputLimit,
        Some("context_length_exceeded") => FinishOutcome::ContextLimit,
        Some("content_filter") | Some("safety") => FinishOutcome::Safety,
        Some("cancelled") => FinishOutcome::Cancelled,
        Some("network_error") => FinishOutcome::NetworkInterruptionAfterVisibleOutput,
        Some(value) => FinishOutcome::UnknownProviderValue {
            raw: value.to_owned(),
        },
        None => FinishOutcome::UnknownProviderValue { raw: String::new() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_id() -> ProviderRequestId {
        ProviderRequestId::new("request-1").unwrap()
    }

    #[test]
    fn reasoning_precedes_content_and_parallel_tool_indexes_do_not_collide() {
        let mut state = AttemptState::default();
        let mut tools = BTreeMap::new();
        tools.insert("read_file".into(), ToolId::new("read").unwrap());
        tools.insert("write_file".into(), ToolId::new("write").unwrap());
        let events = state
            .accept_data(
                r#"{"choices":[{"delta":{"reasoning_content":"think","content":"answer","tool_calls":[{"index":1,"id":"call-2","function":{"name":"write_","arguments":"{\"path\":"}},{"index":0,"id":"call-1","function":{"name":"read_","arguments":"{\"path\":"}}]}}]}"#,
                &request_id(),
                &tools,
            )
            .unwrap();
        assert!(matches!(
            events[0],
            ProviderStreamEvent::ReasoningDelta { .. }
        ));
        assert!(matches!(
            events[1],
            ProviderStreamEvent::ContentDelta { .. }
        ));
        state
            .accept_data(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"file","arguments":"\"a\"}"}},{"index":1,"function":{"name":"file","arguments":"\"b\"}"}}]},"finish_reason":"tool_calls"}]}"#,
                &request_id(),
                &tools,
            )
            .unwrap();
        let completed = state.complete_tool_events(&request_id(), &tools).unwrap();
        assert_eq!(completed.len(), 2);
        let ProviderStreamEvent::ToolCallCompleted(first) = &completed[0] else {
            panic!("expected completed tool");
        };
        assert_eq!(first.tool_id, ToolId::new("read").unwrap());
        assert_eq!(first.arguments, json!({"path": "a"}));
    }

    #[test]
    fn missing_usage_is_unavailable_not_zero() {
        let usage = normalize_usage(&json!({"prompt_tokens": 9})).unwrap();
        assert_eq!(usage.input, UsageMeasurement::exact(9));
        assert_eq!(usage.output, UsageMeasurement::unavailable());
        assert_eq!(usage.total, UsageMeasurement::unavailable());
    }

    #[test]
    fn malformed_json_is_ignored_like_frozen_source() {
        let mut state = AttemptState::default();
        assert!(
            state
                .accept_data("{broken", &request_id(), &BTreeMap::new())
                .unwrap()
                .is_empty()
        );
    }
}
