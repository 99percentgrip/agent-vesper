use std::{collections::BTreeMap, path::PathBuf};

use agent_client_protocol::schema::{
    ProtocolVersion,
    v1::{
        AgentCapabilities, AuthMethod, AuthMethodTerminal, ContentBlock, ContentChunk,
        Implementation, InitializeResponse, PromptCapabilities,
        SessionAdditionalDirectoriesCapabilities, SessionCapabilities, SessionCloseCapabilities,
        SessionForkCapabilities, SessionListCapabilities, SessionNotification,
        SessionResumeCapabilities, SessionUpdate, StopReason, TextContent, ToolCall as AcpToolCall,
        ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, UsageUpdate,
    },
};
use vesper_domain::{
    ContentPart, FinishOutcome, HarnessEvent, HarnessEventPayload, ImageDescriptor, MediaSource,
    MessageId, SessionId, TurnId, WorkspaceRoot,
};

/// Truthful Stage 4 initialization response.
#[must_use]
pub fn truthful_initialize_response(protocol: ProtocolVersion) -> InitializeResponse {
    InitializeResponse::new(protocol)
        .agent_info(Implementation::new(
            "agent-vesper",
            env!("CARGO_PKG_VERSION"),
        ))
        .auth_methods(vec![AuthMethod::Terminal(
            AuthMethodTerminal::new("zai-api-key-setup", "Z.ai API key setup")
                .description("Validates an already configured Z.ai API key; writes no credentials"),
        )])
        .agent_capabilities(
            AgentCapabilities::new()
                .load_session(true)
                .prompt_capabilities(
                    PromptCapabilities::new()
                        .image(true)
                        .audio(false)
                        .embedded_context(true),
                )
                .session_capabilities(
                    SessionCapabilities::new()
                        .list(SessionListCapabilities::new())
                        .additional_directories(SessionAdditionalDirectoriesCapabilities::new())
                        .fork(SessionForkCapabilities::new())
                        .resume(SessionResumeCapabilities::new())
                        .close(SessionCloseCapabilities::new()),
                ),
        )
}

pub(crate) fn workspace_roots(cwd: PathBuf, additional: Vec<PathBuf>) -> Vec<WorkspaceRoot> {
    let mut roots = Vec::with_capacity(additional.len() + 1);
    roots.push(WorkspaceRoot {
        name: vesper_domain::BoundedString::new("workspace").expect("static workspace name"),
        path: vesper_domain::BoundedString::new(cwd.to_string_lossy().into_owned())
            .expect("ACP path bound"),
        primary: true,
    });
    roots.extend(additional.into_iter().enumerate().map(|(index, path)| {
        WorkspaceRoot {
            name: vesper_domain::BoundedString::new(format!("additional-{index}"))
                .expect("bounded root name"),
            path: vesper_domain::BoundedString::new(path.to_string_lossy().into_owned())
                .expect("ACP path bound"),
            primary: false,
        }
    }));
    roots
}

pub(crate) fn content_from_acp(
    blocks: Vec<ContentBlock>,
) -> Result<Vec<ContentPart>, &'static str> {
    blocks
        .into_iter()
        .map(|block| match block {
            ContentBlock::Text(value) => vesper_domain::ContentText::new(value.text)
                .map(ContentPart::Text)
                .map_err(|_| "prompt text exceeds runtime bound"),
            ContentBlock::Image(value) => {
                if value.data.len() > 4 * 1024 * 1024 {
                    return Err("image data exceeds runtime bound");
                }
                let reference = format!("data:{};base64,{}", value.mime_type, value.data);
                Ok(ContentPart::Image(ImageDescriptor {
                    media_type: value.mime_type,
                    source: MediaSource::Reference { reference },
                    alt_text: None,
                }))
            }
            ContentBlock::ResourceLink(value) => Ok(ContentPart::EmbeddedContext(
                vesper_domain::EmbeddedContextReference {
                    source: "acp-resource-link".into(),
                    reference: value.uri,
                    provider_visible: true,
                },
            )),
            ContentBlock::Resource(value) => {
                let encoded = serde_json::to_string(&value)
                    .map_err(|_| "embedded context cannot be represented")?;
                Ok(ContentPart::EmbeddedContext(
                    vesper_domain::EmbeddedContextReference {
                        source: "acp-embedded-resource".into(),
                        reference: encoded,
                        provider_visible: true,
                    },
                ))
            }
            ContentBlock::Audio(_) => Err("audio prompts are unsupported"),
            _ => Err("ACP content type is unsupported"),
        })
        .collect()
}

pub(crate) fn message_id_from_meta(
    meta: Option<&agent_client_protocol::schema::v1::Meta>,
    fallback: impl FnOnce() -> String,
) -> Result<MessageId, &'static str> {
    let value = meta
        .and_then(|map| map.get("userMessageId"))
        .and_then(serde_json::Value::as_str)
        .map_or_else(fallback, str::to_owned);
    MessageId::new(value).map_err(|_| "user message ID is invalid")
}

pub(crate) fn stop_reason(outcome: &FinishOutcome) -> StopReason {
    match outcome {
        FinishOutcome::Cancelled => StopReason::Cancelled,
        FinishOutcome::OutputLimit
        | FinishOutcome::ContextLimit
        | FinishOutcome::NetworkInterruptionAfterVisibleOutput
        | FinishOutcome::ProviderError
        | FinishOutcome::ProtocolError
        | FinishOutcome::Safety
        | FinishOutcome::UnknownProviderValue { .. } => StopReason::Refusal,
        FinishOutcome::Stop | FinishOutcome::ToolCalls => StopReason::EndTurn,
    }
}

#[derive(Debug, Default)]
struct ToolState {
    acp_id: String,
    provider_id: String,
    name: String,
}

/// Stateful event-to-update mapper. State is bounded by the number of active
/// tool calls and discarded when each call completes.
pub(crate) struct AcpEventMapper {
    context_window: u64,
    tools: BTreeMap<(SessionId, u32), ToolState>,
}

impl AcpEventMapper {
    pub(crate) fn new(context_window: u64) -> Self {
        Self {
            context_window,
            tools: BTreeMap::new(),
        }
    }

    pub(crate) fn notification(&mut self, event: &HarnessEvent) -> Option<SessionNotification> {
        let session = event.session_id.as_ref()?;
        let session_id = agent_client_protocol::schema::v1::SessionId::new(session.as_str());
        let update = match &event.payload {
            HarnessEventPayload::ReasoningDelta { text, .. } => SessionUpdate::AgentThoughtChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(text.as_str()))),
            ),
            HarnessEventPayload::ContentDelta {
                part: ContentPart::Text(text),
                ..
            } => SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new(text.as_str()),
            ))),
            HarnessEventPayload::UsageUpdated(usage) => {
                let used = usage
                    .total
                    .value
                    .or_else(|| {
                        usage
                            .input
                            .value
                            .zip(usage.output.value)
                            .and_then(|(a, b)| a.checked_add(b))
                    })
                    .unwrap_or(0);
                SessionUpdate::UsageUpdate(UsageUpdate::new(used, self.context_window))
            }
            HarnessEventPayload::ToolCallStarted {
                index,
                call_id,
                name,
            } => {
                let acp_id = call_id.as_ref().map_or_else(
                    || {
                        format!(
                            "vesper-tool:{}:{index}",
                            event.turn_id.as_ref().map_or("turn", TurnId::as_str)
                        )
                    },
                    |value| value.as_str().to_owned(),
                );
                let title = name.as_ref().map_or_else(
                    || "provider tool call".to_owned(),
                    |value| value.as_str().to_owned(),
                );
                self.tools.insert(
                    (session.clone(), *index),
                    ToolState {
                        acp_id: acp_id.clone(),
                        provider_id: call_id
                            .as_ref()
                            .map_or_else(String::new, |value| value.as_str().to_owned()),
                        name: title.clone(),
                    },
                );
                SessionUpdate::ToolCall(AcpToolCall::new(acp_id, title))
            }
            HarnessEventPayload::ToolCallUpdated {
                index,
                id_fragment,
                name_fragment,
                arguments_fragment,
            } => {
                let state = self
                    .tools
                    .entry((session.clone(), *index))
                    .or_insert_with(|| ToolState {
                        acp_id: format!(
                            "vesper-tool:{}:{index}",
                            event.turn_id.as_ref().map_or("turn", TurnId::as_str)
                        ),
                        provider_id: String::new(),
                        name: "provider tool call".to_owned(),
                    });
                if let Some(fragment) = id_fragment {
                    state.provider_id.push_str(fragment.as_str());
                }
                if let Some(fragment) = name_fragment {
                    if state.name == "provider tool call" {
                        state.name.clear();
                    }
                    state.name.push_str(fragment.as_str());
                }
                let fields = ToolCallUpdateFields::new()
                    .title(state.name.clone())
                    .raw_input(serde_json::json!({
                        "argumentsFragment": arguments_fragment.as_str()
                    }));
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(state.acp_id.clone(), fields))
            }
            HarnessEventPayload::ToolCallCompleted(call) => {
                let key = self
                    .tools
                    .iter()
                    .find(|((candidate, _), state)| {
                        candidate == session && state.provider_id == call.id.as_str()
                    })
                    .map(|(key, _)| key.clone())
                    .or_else(|| {
                        self.tools
                            .keys()
                            .find(|(candidate, _)| candidate == session)
                            .cloned()
                    });
                let state = key
                    .and_then(|key| self.tools.remove(&key))
                    .unwrap_or_else(|| ToolState {
                        acp_id: call.id.as_str().to_owned(),
                        provider_id: call.id.as_str().to_owned(),
                        name: call.tool_id.as_str().to_owned(),
                    });
                let fields = ToolCallUpdateFields::new()
                    .title(call.tool_id.as_str().to_owned())
                    .status(ToolCallStatus::Failed)
                    .raw_input(call.arguments.clone())
                    .raw_output(serde_json::json!({
                        "error": "tool execution is unavailable in the minimal runtime"
                    }));
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(state.acp_id, fields))
            }
            HarnessEventPayload::Warning { message } => {
                SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                    TextContent::new(format!("Warning: {}", message.as_str())),
                )))
            }
            HarnessEventPayload::RecoverableError(error) => {
                SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                    TextContent::new(format!("Error: {}", error.safe_message.as_str())),
                )))
            }
            HarnessEventPayload::UserMessageAccepted { .. }
            | HarnessEventPayload::ResponseStarted { .. }
            | HarnessEventPayload::ToolResultCompleted(_)
            | HarnessEventPayload::ToolCallFailed { .. }
            | HarnessEventPayload::PermissionRequested { .. }
            | HarnessEventPayload::PermissionResolved { .. }
            | HarnessEventPayload::PlanChanged(_)
            | HarnessEventPayload::GoalChanged(_)
            | HarnessEventPayload::ProviderStatusUpdated { .. }
            | HarnessEventPayload::ContextPressureUpdated { .. }
            | HarnessEventPayload::FallbackApplied { .. }
            | HarnessEventPayload::RuntimeInitialized { .. }
            | HarnessEventPayload::SessionCreated { .. }
            | HarnessEventPayload::SessionLoaded { .. }
            | HarnessEventPayload::SessionListProduced { .. }
            | HarnessEventPayload::SessionMetadataChanged { .. }
            | HarnessEventPayload::TurnCompleted { .. }
            | HarnessEventPayload::TurnCancelled { .. }
            | HarnessEventPayload::SessionClosed
            | HarnessEventPayload::RuntimeShutdown
            | HarnessEventPayload::ContentDelta { .. } => return None,
        };
        Some(SessionNotification::new(session_id, update))
    }
}

pub(crate) fn session_id(value: &agent_client_protocol::schema::v1::SessionId) -> SessionId {
    SessionId::new(value.to_string()).expect("ACP session ID is schema validated")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthful_capabilities_do_not_advertise_audio_or_mcp() {
        let value =
            serde_json::to_value(truthful_initialize_response(ProtocolVersion::V1)).unwrap();
        assert_eq!(value["protocolVersion"], 1);
        assert_eq!(
            value["agentCapabilities"]["promptCapabilities"]["image"],
            true
        );
        assert_eq!(
            value["agentCapabilities"]["promptCapabilities"]["audio"],
            false
        );
        let mcp = &value["agentCapabilities"]["mcpCapabilities"];
        assert!(
            mcp.is_null()
                || mcp.as_object().is_some_and(|fields| {
                    fields.is_empty()
                        || fields
                            .values()
                            .all(|value| value == &serde_json::Value::Bool(false))
                }),
            "unexpected MCP capability advertisement: {mcp}"
        );
    }
}
