use std::{collections::BTreeSet, fmt};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use vesper_domain::{
    BoundedString, ContentPart, ContentText, ConversationMessage, EndpointId, ExtensionMap,
    ExtensionNamespace, ImageDescriptor, MediaSource, MessageId, MessageRole, ModelId,
    NormalizedUsage, OpaqueContent, OpaqueProviderData, ProviderId, QualifiedModelId,
    ReasoningBlock, ReasoningKind, ReasoningRetention, Revision, SchemaVersion, SessionId,
    SessionLineage, SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId, ToolId,
    ToolResult, ToolResultId, ToolResultStatus, UsageMeasurement, UsageMode,
    VersionedExtensionEnvelope, WorkspaceRoot,
};

use crate::{
    AvailableCommandDescriptor, DecodedLegacySession, ReplayMessage, ReplayMetadata, ReplayPlan,
    ReplayPlanEntry, ReplayPlanPriority, ReplayPlanStatus, SessionSource,
};

/// Provider/model/endpoint availability supplied by the future composition boundary.
#[derive(Debug, Clone, Default)]
pub struct CompatibilityAvailability {
    providers: BTreeSet<ProviderId>,
    models: BTreeSet<QualifiedModelId>,
    endpoints: BTreeSet<(ProviderId, EndpointId)>,
}

impl CompatibilityAvailability {
    #[must_use]
    pub fn with_provider(mut self, provider: ProviderId) -> Self {
        self.providers.insert(provider);
        self
    }

    #[must_use]
    pub fn with_model(mut self, model: QualifiedModelId) -> Self {
        self.models.insert(model);
        self
    }

    #[must_use]
    pub fn with_endpoint(mut self, provider: ProviderId, endpoint: EndpointId) -> Self {
        self.endpoints.insert((provider, endpoint));
        self
    }

    #[must_use]
    pub fn status_for(
        &self,
        provider: &ProviderId,
        model: &QualifiedModelId,
        endpoint: &EndpointId,
    ) -> SessionConfigurationStatus {
        let mut issues = Vec::new();
        if !self.providers.contains(provider) {
            issues.push(ConfigurationIssue::UnknownProvider(provider.clone()));
        }
        if !self.models.contains(model) {
            issues.push(ConfigurationIssue::UnknownModel(model.clone()));
        }
        if !self
            .endpoints
            .contains(&(provider.clone(), endpoint.clone()))
        {
            issues.push(ConfigurationIssue::UnavailableEndpoint(endpoint.clone()));
        }
        if issues.is_empty() {
            SessionConfigurationStatus::Ready
        } else {
            SessionConfigurationStatus::ConfigurationRequired(issues)
        }
    }
}

/// Why a restored session is replayable but cannot start a provider turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurationIssue {
    UnknownProvider(ProviderId),
    UnknownModel(QualifiedModelId),
    UnavailableEndpoint(EndpointId),
}

/// Provider-dispatch readiness of one converted session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionConfigurationStatus {
    Ready,
    ConfigurationRequired(Vec<ConfigurationIssue>),
}

impl SessionConfigurationStatus {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Full frozen record retained for a future explicit writer. Debug output is
/// redacted because it can contain reasoning and private message bodies.
#[derive(Clone, PartialEq)]
pub struct LegacyCompatibilityData {
    record: vesper_domain::LegacySessionV1,
}

impl LegacyCompatibilityData {
    #[must_use]
    pub fn expose_for_compatibility(&self) -> &vesper_domain::LegacySessionV1 {
        &self.record
    }
}

impl fmt::Debug for LegacyCompatibilityData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LegacyCompatibilityData(<redacted>)")
    }
}

/// Compatibility payload retained according to the record format.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCompatibilityData {
    Legacy(Box<LegacyCompatibilityData>),
    AgentVesper(VersionedExtensionEnvelope),
}

/// Pure converted state accepted later by `vesper-runtime`.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistedSessionState {
    pub session_id: SessionId,
    pub source: SessionSource,
    pub lineage: SessionLineage,
    pub workspace_roots: Vec<WorkspaceRoot>,
    pub provider_id: ProviderId,
    pub model: QualifiedModelId,
    pub endpoint_id: EndpointId,
    pub provider_configuration: PersistedProviderConfiguration,
    pub configuration_status: SessionConfigurationStatus,
    pub operating_mode: SessionOperatingMode,
    pub permission_mode: SessionPermissionMode,
    pub history: Vec<ConversationMessage>,
    pub cumulative_usage: NormalizedUsage,
    pub revision: Revision,
    pub replay: ReplayPlan,
    pub compatibility: SessionCompatibilityData,
}

/// Provider-owned compatibility configuration without an adapter dependency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedProviderConfiguration {
    pub provider_id: ProviderId,
    pub values: VersionedExtensionEnvelope,
}

/// Pure compatibility conversion failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SessionConversionError {
    #[error("legacy session contains an invalid bounded identity")]
    Identity,
    #[error("legacy session contains invalid compatibility configuration")]
    Configuration,
    #[error("legacy session content cannot be represented safely")]
    Content,
}

/// Read/write-free converter for already decoded schema-v1 records.
#[derive(Debug, Clone)]
pub struct LegacyRuntimeConverter {
    availability: CompatibilityAvailability,
}

impl LegacyRuntimeConverter {
    #[must_use]
    pub fn new(availability: CompatibilityAvailability) -> Self {
        Self { availability }
    }

    pub fn convert(
        &self,
        decoded: DecodedLegacySession,
    ) -> Result<PersistedSessionState, SessionConversionError> {
        let session_id = decoded.metadata.session_id.clone();
        let record = decoded.session;
        let provider_id = legacy_provider(&record, &decoded.metadata)?;
        let model = QualifiedModelId {
            provider_id: provider_id.clone(),
            model_id: ModelId::new(record.model.clone())
                .map_err(|_| SessionConversionError::Identity)?,
        };
        let endpoint_id = legacy_endpoint(&record.api_endpoint)?;
        let lineage = legacy_lineage(&record, &session_id)?;
        let operating_mode = legacy_mode(&record.mode)?;
        let permission_mode = legacy_permission(&record.permission_mode)?;
        let workspace_roots = legacy_roots(&record)?;
        let (history, visible_messages) = convert_messages(&record, &session_id, &provider_id)?;
        let replay_entries = convert_plan(&record.plan)?;
        let provider_configuration = legacy_provider_configuration(&record, provider_id.clone())?;
        let configuration_status = self
            .availability
            .status_for(&provider_id, &model, &endpoint_id);
        let replay = ReplayPlan::new(
            visible_messages,
            replay_entries,
            ReplayMetadata {
                title: decoded
                    .metadata
                    .title
                    .map(BoundedString::new)
                    .transpose()
                    .map_err(|_| SessionConversionError::Content)?,
                updated_at: decoded
                    .metadata
                    .updated_at
                    .map(BoundedString::new)
                    .transpose()
                    .map_err(|_| SessionConversionError::Content)?,
                operating_mode,
                configuration_required: !configuration_status.is_ready(),
            },
            // Stage 4 has no supported slash-command implementation; an empty
            // catalog is truthful and is still replayed as an update.
            Vec::<AvailableCommandDescriptor>::new(),
        );
        Ok(PersistedSessionState {
            session_id,
            source: decoded.metadata.source,
            lineage,
            workspace_roots,
            provider_id,
            model,
            endpoint_id,
            provider_configuration,
            configuration_status,
            operating_mode,
            permission_mode,
            history,
            cumulative_usage: legacy_usage(&record),
            revision: Revision::new(0),
            replay,
            compatibility: SessionCompatibilityData::Legacy(Box::new(LegacyCompatibilityData {
                record,
            })),
        })
    }
}

fn legacy_provider(
    record: &vesper_domain::LegacySessionV1,
    metadata: &crate::SessionMetadata,
) -> Result<ProviderId, SessionConversionError> {
    let value = record
        .unknown_fields
        .get("provider")
        .and_then(Value::as_str)
        .or(metadata.provider.as_deref())
        .unwrap_or("zai");
    ProviderId::new(value).map_err(|_| SessionConversionError::Identity)
}

fn legacy_endpoint(value: &str) -> Result<EndpointId, SessionConversionError> {
    let value = match value {
        "coding" => "zai-coding",
        "standard" => "zai-standard",
        "bigmodel" => "zai-bigmodel-cn",
        "custom" => "zai-custom",
        other => other,
    };
    EndpointId::new(value).map_err(|_| SessionConversionError::Identity)
}

fn legacy_lineage(
    record: &vesper_domain::LegacySessionV1,
    session_id: &SessionId,
) -> Result<SessionLineage, SessionConversionError> {
    Ok(SessionLineage {
        root_session_id: SessionId::new(
            record
                .branch_root_id
                .as_deref()
                .unwrap_or(session_id.as_str()),
        )
        .map_err(|_| SessionConversionError::Identity)?,
        parent_session_id: record
            .parent_session_id
            .as_deref()
            .map(SessionId::new)
            .transpose()
            .map_err(|_| SessionConversionError::Identity)?,
    })
}

fn legacy_mode(value: &str) -> Result<SessionOperatingMode, SessionConversionError> {
    match value {
        "code" => Ok(SessionOperatingMode::Code),
        "plan" => Ok(SessionOperatingMode::Plan),
        _ => Err(SessionConversionError::Configuration),
    }
}

fn legacy_permission(value: &str) -> Result<SessionPermissionMode, SessionConversionError> {
    match value {
        "ask" => Ok(SessionPermissionMode::Ask),
        "bypass" => Ok(SessionPermissionMode::Bypass),
        "read-only" => Ok(SessionPermissionMode::ReadOnly),
        _ => Err(SessionConversionError::Configuration),
    }
}

fn legacy_roots(
    record: &vesper_domain::LegacySessionV1,
) -> Result<Vec<WorkspaceRoot>, SessionConversionError> {
    let mut roots = vec![WorkspaceRoot {
        name: BoundedString::new("workspace").map_err(|_| SessionConversionError::Content)?,
        path: BoundedString::new(record.cwd.clone())
            .map_err(|_| SessionConversionError::Content)?,
        primary: true,
    }];
    if let Some(Value::Array(additional)) = record.unknown_fields.get("additional_directories") {
        for (index, value) in additional.iter().enumerate() {
            let Some(path) = value.as_str() else {
                continue;
            };
            roots.push(WorkspaceRoot {
                name: BoundedString::new(format!("additional-{index}"))
                    .map_err(|_| SessionConversionError::Content)?,
                path: BoundedString::new(path).map_err(|_| SessionConversionError::Content)?,
                primary: false,
            });
        }
    }
    Ok(roots)
}

fn legacy_provider_configuration(
    record: &vesper_domain::LegacySessionV1,
    provider_id: ProviderId,
) -> Result<PersistedProviderConfiguration, SessionConversionError> {
    let mut values = ExtensionMap::default();
    for (key, value) in [
        ("zai:model", json!(record.model)),
        ("zai:endpoint-plan", json!(record.api_endpoint)),
        ("zai:reasoning-mode", json!(record.thought_level)),
        ("zai:generation-profile", json!(record.generation_profile)),
        ("zai:auxiliary-model", json!(record.auxiliary_model)),
    ] {
        values
            .insert(key, value)
            .map_err(|_| SessionConversionError::Configuration)?;
    }
    Ok(PersistedProviderConfiguration {
        provider_id,
        values: VersionedExtensionEnvelope {
            namespace: ExtensionNamespace::new("provider.zai")
                .map_err(|_| SessionConversionError::Configuration)?,
            version: SchemaVersion::new(1).ok_or(SessionConversionError::Configuration)?,
            values,
        },
    })
}

fn legacy_usage(record: &vesper_domain::LegacySessionV1) -> NormalizedUsage {
    let mut usage = NormalizedUsage::unavailable(UsageMode::Cumulative);
    usage.input = UsageMeasurement::exact(record.total_input_tokens);
    usage.output = UsageMeasurement::exact(record.total_output_tokens);
    usage.cached_input = UsageMeasurement::exact(record.total_cached_tokens);
    usage.total = record
        .total_input_tokens
        .checked_add(record.total_output_tokens)
        .map_or_else(UsageMeasurement::unavailable, UsageMeasurement::exact);
    usage
}

fn convert_messages(
    record: &vesper_domain::LegacySessionV1,
    session_id: &SessionId,
    provider: &ProviderId,
) -> Result<(Vec<ConversationMessage>, Vec<ReplayMessage>), SessionConversionError> {
    let known_calls = collect_tool_call_ids(&record.messages);
    let mut history = Vec::new();
    let mut replay = Vec::new();
    for (ordinal, value) in record.messages.iter().enumerate() {
        let Some(fields) = value.as_object() else {
            continue;
        };
        let Some(role) = fields.get("role").and_then(Value::as_str) else {
            continue;
        };
        let message_id = message_identity(fields, session_id, ordinal, role)?;
        match role {
            "user" | "assistant" => {
                let role = if role == "user" {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                };
                let mut content = active_content(fields)?;
                if role == MessageRole::Assistant {
                    content.extend(convert_tool_calls(fields, session_id, ordinal)?);
                    if let Some(reasoning) = fields
                        .get("reasoning_content")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                    {
                        content.push(ContentPart::Reasoning(ReasoningBlock {
                            kind: ReasoningKind::OpaqueContinuation,
                            retention: ReasoningRetention::Persist,
                            text: None,
                            opaque: Some(OpaqueContent {
                                provider_id: provider.clone(),
                                kind: "legacy-reasoning-content".into(),
                                data: OpaqueProviderData::new(json!({"value": reasoning}))
                                    .map_err(|_| SessionConversionError::Content)?,
                            }),
                        }));
                    }
                }
                if let Some(text) = replay_text(fields)? {
                    replay.push(ReplayMessage {
                        message_id: message_id.clone(),
                        role: role.clone(),
                        text,
                    });
                }
                if !content.is_empty() {
                    history.push(ConversationMessage {
                        id: message_id,
                        role,
                        content,
                        extensions: ExtensionMap::default(),
                    });
                }
            }
            "tool" => {
                let Some(call_id) = fields
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .filter(|id| known_calls.contains(*id))
                else {
                    continue;
                };
                let call_id =
                    ToolCallId::new(call_id).map_err(|_| SessionConversionError::Identity)?;
                history.push(ConversationMessage {
                    id: message_id,
                    role: MessageRole::Tool,
                    content: vec![ContentPart::ToolResult(ToolResult {
                        id: ToolResultId::new(deterministic_id(
                            "legacy-tool-result",
                            session_id,
                            ordinal,
                            "tool",
                        ))
                        .map_err(|_| SessionConversionError::Identity)?,
                        call_id,
                        output: fields.get("content").cloned().unwrap_or(Value::Null),
                        status: ToolResultStatus::Succeeded,
                        locations: Vec::new(),
                        diff_summary: None,
                        extensions: ExtensionMap::default(),
                    })],
                    extensions: ExtensionMap::default(),
                });
            }
            _ => {}
        }
    }
    Ok((history, replay))
}

fn collect_tool_call_ids(messages: &[Value]) -> BTreeSet<&str> {
    messages
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|fields| fields.get("tool_calls").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|call| call.get("id").and_then(Value::as_str))
        .collect()
}

fn message_identity(
    fields: &Map<String, Value>,
    session_id: &SessionId,
    ordinal: usize,
    role: &str,
) -> Result<MessageId, SessionConversionError> {
    if let Some(existing) = fields.get("id").and_then(Value::as_str) {
        return MessageId::new(existing).map_err(|_| SessionConversionError::Identity);
    }
    MessageId::new(deterministic_id(
        "legacy-message",
        session_id,
        ordinal,
        role,
    ))
    .map_err(|_| SessionConversionError::Identity)
}

fn deterministic_id(prefix: &str, session_id: &SessionId, ordinal: usize, role: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"agent-vesper-legacy-identity-v1\0");
    digest.update(session_id.as_str().as_bytes());
    digest.update(b"\0");
    digest.update(ordinal.to_be_bytes());
    digest.update(b"\0");
    digest.update(role.as_bytes());
    let bytes = digest.finalize();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    format!("{prefix}-{encoded}")
}

fn active_content(fields: &Map<String, Value>) -> Result<Vec<ContentPart>, SessionConversionError> {
    match fields.get("content") {
        Some(Value::String(text)) if !text.is_empty() => Ok(vec![ContentPart::Text(
            ContentText::new(text).map_err(|_| SessionConversionError::Content)?,
        )]),
        Some(Value::String(_)) => Ok(Vec::new()),
        Some(Value::Array(blocks)) => {
            let mut content = Vec::new();
            for block in blocks.iter().filter_map(Value::as_object) {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            content.push(ContentPart::Text(
                                ContentText::new(text)
                                    .map_err(|_| SessionConversionError::Content)?,
                            ));
                        }
                    }
                    Some("image_url") => {
                        if let Some(reference) = block
                            .get("image_url")
                            .and_then(Value::as_object)
                            .and_then(|image| image.get("url"))
                            .and_then(Value::as_str)
                        {
                            content.push(ContentPart::Image(ImageDescriptor {
                                media_type: media_type_from_reference(reference),
                                source: MediaSource::Reference {
                                    reference: reference.to_owned(),
                                },
                                alt_text: None,
                            }));
                        }
                    }
                    _ => {}
                }
            }
            Ok(content)
        }
        _ => Ok(Vec::new()),
    }
}

fn media_type_from_reference(reference: &str) -> String {
    reference
        .strip_prefix("data:")
        .and_then(|value| value.split_once(';').map(|(media_type, _)| media_type))
        .unwrap_or("application/octet-stream")
        .to_owned()
}

fn replay_text(
    fields: &Map<String, Value>,
) -> Result<Option<BoundedString<1_048_576>>, SessionConversionError> {
    let text = match fields.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(Value::as_object)
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };
    if text.is_empty() {
        Ok(None)
    } else {
        BoundedString::new(text)
            .map(Some)
            .map_err(|_| SessionConversionError::Content)
    }
}

fn convert_tool_calls(
    fields: &Map<String, Value>,
    session_id: &SessionId,
    message_ordinal: usize,
) -> Result<Vec<ContentPart>, SessionConversionError> {
    let Some(calls) = fields.get("tool_calls").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    calls
        .iter()
        .enumerate()
        .filter_map(|(index, value)| value.as_object().map(|value| (index, value)))
        .map(|(index, call)| {
            let function = call.get("function").and_then(Value::as_object);
            let name = function
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("legacy-unknown-tool");
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    deterministic_id(
                        "legacy-tool-call",
                        session_id,
                        message_ordinal,
                        &format!("assistant-{index}"),
                    )
                });
            let arguments = function
                .and_then(|value| value.get("arguments"))
                .cloned()
                .unwrap_or(Value::Object(Map::new()));
            let arguments = match arguments {
                Value::String(value) => {
                    serde_json::from_str(&value).unwrap_or(Value::String(value))
                }
                value => value,
            };
            Ok(ContentPart::ToolCall(ToolCall {
                id: ToolCallId::new(id).map_err(|_| SessionConversionError::Identity)?,
                tool_id: ToolId::new(name).map_err(|_| SessionConversionError::Identity)?,
                arguments,
                extensions: ExtensionMap::default(),
            }))
        })
        .collect()
}

fn convert_plan(values: &[Value]) -> Result<Vec<ReplayPlanEntry>, SessionConversionError> {
    values
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|entry| entry.get("content").map(|content| (entry, content)))
        .map(|(entry, content)| {
            let content = content
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| content.to_string());
            Ok(ReplayPlanEntry {
                content: BoundedString::new(content)
                    .map_err(|_| SessionConversionError::Content)?,
                status: match entry.get("status").and_then(Value::as_str) {
                    Some("completed") => ReplayPlanStatus::Completed,
                    Some("in_progress") => ReplayPlanStatus::InProgress,
                    _ => ReplayPlanStatus::Pending,
                },
                priority: match entry.get("priority").and_then(Value::as_str) {
                    Some("high") => ReplayPlanPriority::High,
                    Some("low") => ReplayPlanPriority::Low,
                    _ => ReplayPlanPriority::Medium,
                },
            })
        })
        .collect()
}
