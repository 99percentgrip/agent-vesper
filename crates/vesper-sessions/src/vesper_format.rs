use serde::{Deserialize, Serialize};
use vesper_domain::{
    BoundedString, ContentPart, ConversationMessage, EndpointId, MessageRole, NormalizedUsage,
    ProviderId, QualifiedModelId, Revision, SessionId, SessionLineage, SessionOperatingMode,
    SessionPermissionMode, VersionedExtensionEnvelope, WorkspaceRoot,
};

use crate::{
    BoundViolation, CompatibilityAvailability, CorruptLegacyRecord, PersistedProviderConfiguration,
    PersistedSessionState, ReplayMessage, ReplayMetadata, ReplayPlan, ReplayPlanEntry,
    SessionCompatibilityData, SessionMetadata, SessionReader, SessionSource, SessionStoreError,
};

const VESPER_SESSION_FORMAT: &str = "agent-vesper-session";
const VESPER_SESSION_VERSION: u32 = 1;

/// Version-1 Agent Vesper session record. This is a read-only format contract;
/// no production writer is exposed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VesperSessionV1 {
    pub format: BoundedString<64>,
    pub version: u32,
    pub session_id: SessionId,
    pub title: Option<BoundedString<1024>>,
    pub updated_at: Option<BoundedString<128>>,
    pub lineage: SessionLineage,
    pub workspace_roots: Vec<WorkspaceRoot>,
    pub provider_id: ProviderId,
    pub model: QualifiedModelId,
    pub endpoint_id: EndpointId,
    pub provider_configuration: PersistedProviderConfiguration,
    pub operating_mode: SessionOperatingMode,
    pub permission_mode: SessionPermissionMode,
    pub history: Vec<ConversationMessage>,
    pub cumulative_usage: NormalizedUsage,
    pub revision: Revision,
    #[serde(default)]
    pub plan: Vec<ReplayPlanEntry>,
    pub extensions: VersionedExtensionEnvelope,
}

impl VesperSessionV1 {
    /// Returns the required format discriminator.
    #[must_use]
    pub const fn format_name() -> &'static str {
        VESPER_SESSION_FORMAT
    }

    /// Returns the only currently supported version.
    #[must_use]
    pub const fn current_version() -> u32 {
        VESPER_SESSION_VERSION
    }
}

/// Bounds applied after the filesystem byte bound and before runtime adoption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VesperDecodeBounds {
    pub max_file_bytes: usize,
    pub max_messages: usize,
    pub max_roots: usize,
    pub max_plan_items: usize,
}

impl Default for VesperDecodeBounds {
    fn default() -> Self {
        Self {
            max_file_bytes: 16 * 1024 * 1024,
            max_messages: 10_000,
            max_roots: 128,
            max_plan_items: 1_000,
        }
    }
}

/// Typed read-only Agent Vesper decode outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum VesperLoadOutcome {
    Loaded(Box<PersistedSessionState>),
    Missing,
    Corrupt(CorruptLegacyRecord),
    UnsupportedVersion(u32),
    RejectedByBounds(BoundViolation),
    PermissionDenied,
    UnsafePath,
}

/// Stateless reader for the future Agent Vesper format.
#[derive(Debug, Clone)]
pub struct VesperSessionDecoder {
    bounds: VesperDecodeBounds,
    availability: CompatibilityAvailability,
}

impl VesperSessionDecoder {
    #[must_use]
    pub fn new(bounds: VesperDecodeBounds, availability: CompatibilityAvailability) -> Self {
        Self {
            bounds,
            availability,
        }
    }

    pub async fn load(
        &self,
        reader: &dyn SessionReader,
        session_id: &SessionId,
    ) -> VesperLoadOutcome {
        match reader.load(session_id).await {
            Ok(Some(record)) => self.decode_record(record.metadata, &record.bytes),
            Ok(None) => VesperLoadOutcome::Missing,
            Err(error) => classify_store_error(error),
        }
    }

    #[must_use]
    pub fn decode_record(&self, metadata: SessionMetadata, bytes: &[u8]) -> VesperLoadOutcome {
        if bytes.len() > self.bounds.max_file_bytes {
            return rejected("file_bytes", self.bounds.max_file_bytes);
        }
        let value: serde_json::Value = match serde_json::from_slice(bytes) {
            Ok(value) => value,
            Err(_) => return VesperLoadOutcome::Corrupt(CorruptLegacyRecord::MalformedJson),
        };
        let Some(fields) = value.as_object() else {
            return VesperLoadOutcome::Corrupt(CorruptLegacyRecord::InvalidShape);
        };
        let version = fields
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok());
        if let Some(version) = version
            && version != VESPER_SESSION_VERSION
        {
            return VesperLoadOutcome::UnsupportedVersion(version);
        }
        let record: VesperSessionV1 = match serde_json::from_value(value) {
            Ok(record) => record,
            Err(_) => return VesperLoadOutcome::Corrupt(CorruptLegacyRecord::InvalidShape),
        };
        if record.format.as_str() != VESPER_SESSION_FORMAT
            || record.version != VESPER_SESSION_VERSION
            || record.session_id != metadata.session_id
            || record.provider_configuration.provider_id != record.provider_id
            || record.model.provider_id != record.provider_id
        {
            return VesperLoadOutcome::Corrupt(CorruptLegacyRecord::CompatibilityValue);
        }
        for (field, actual, maximum) in [
            ("messages", record.history.len(), self.bounds.max_messages),
            (
                "additional_roots",
                record.workspace_roots.len(),
                self.bounds.max_roots,
            ),
            ("plan", record.plan.len(), self.bounds.max_plan_items),
        ] {
            if actual > maximum {
                return rejected(field, maximum);
            }
        }
        if record
            .workspace_roots
            .iter()
            .filter(|root| root.primary)
            .count()
            != 1
        {
            return VesperLoadOutcome::Corrupt(CorruptLegacyRecord::CompatibilityValue);
        }

        let replay_messages = record
            .history
            .iter()
            .filter(|message| matches!(message.role, MessageRole::User | MessageRole::Assistant))
            .flat_map(|message| {
                message.content.iter().filter_map(move |part| {
                    let ContentPart::Text(text) = part else {
                        return None;
                    };
                    if text.as_str().is_empty() {
                        return None;
                    }
                    Some(ReplayMessage {
                        message_id: message.id.clone(),
                        role: message.role.clone(),
                        text: text.clone(),
                    })
                })
            })
            .collect();
        let configuration_status =
            self.availability
                .status_for(&record.provider_id, &record.model, &record.endpoint_id);
        let replay = ReplayPlan::new(
            replay_messages,
            record.plan.clone(),
            ReplayMetadata {
                title: record.title.clone(),
                updated_at: record.updated_at.clone(),
                operating_mode: record.operating_mode,
                configuration_required: !configuration_status.is_ready(),
            },
            Vec::new(),
        );
        VesperLoadOutcome::Loaded(Box::new(PersistedSessionState {
            session_id: record.session_id,
            source: SessionSource::AgentVesper,
            lineage: record.lineage,
            workspace_roots: record.workspace_roots,
            provider_id: record.provider_id,
            model: record.model,
            endpoint_id: record.endpoint_id,
            provider_configuration: record.provider_configuration,
            configuration_status,
            operating_mode: record.operating_mode,
            permission_mode: record.permission_mode,
            history: record.history,
            cumulative_usage: record.cumulative_usage,
            revision: record.revision,
            replay,
            compatibility: SessionCompatibilityData::AgentVesper(record.extensions),
        }))
    }
}

fn rejected(field: &'static str, maximum: usize) -> VesperLoadOutcome {
    VesperLoadOutcome::RejectedByBounds(BoundViolation { field, maximum })
}

fn classify_store_error(error: SessionStoreError) -> VesperLoadOutcome {
    match error {
        SessionStoreError::PathEscapesRoot | SessionStoreError::InvalidFileName(_) => {
            VesperLoadOutcome::UnsafePath
        }
        SessionStoreError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            VesperLoadOutcome::PermissionDenied
        }
        SessionStoreError::RecordLimitExceeded { maximum } => {
            VesperLoadOutcome::RejectedByBounds(BoundViolation {
                field: "file_bytes",
                maximum: usize::try_from(maximum).unwrap_or(usize::MAX),
            })
        }
        _ => VesperLoadOutcome::Corrupt(CorruptLegacyRecord::Unreadable),
    }
}

#[cfg(test)]
mod tests {
    use vesper_domain::{
        ContentText, ExtensionMap, ExtensionNamespace, MessageId, ModelId, SchemaVersion, UsageMode,
    };

    use super::*;

    fn record() -> VesperSessionV1 {
        let provider = ProviderId::new("zai").unwrap();
        VesperSessionV1 {
            format: BoundedString::new(VESPER_SESSION_FORMAT).unwrap(),
            version: VESPER_SESSION_VERSION,
            session_id: SessionId::new("vesper-read-only").unwrap(),
            title: Some(BoundedString::new("Read only fixture").unwrap()),
            updated_at: Some(BoundedString::new("2026-07-30T00:00:00Z").unwrap()),
            lineage: SessionLineage {
                root_session_id: SessionId::new("vesper-read-only").unwrap(),
                parent_session_id: None,
            },
            workspace_roots: vec![WorkspaceRoot {
                name: BoundedString::new("workspace").unwrap(),
                path: BoundedString::new("/fixture").unwrap(),
                primary: true,
            }],
            provider_id: provider.clone(),
            model: QualifiedModelId {
                provider_id: provider.clone(),
                model_id: ModelId::new("glm-5.2").unwrap(),
            },
            endpoint_id: EndpointId::new("zai-coding").unwrap(),
            provider_configuration: PersistedProviderConfiguration {
                provider_id: provider,
                values: envelope("provider.zai"),
            },
            operating_mode: SessionOperatingMode::Code,
            permission_mode: SessionPermissionMode::Ask,
            history: vec![ConversationMessage {
                id: MessageId::new("message-1").unwrap(),
                role: MessageRole::User,
                content: vec![ContentPart::Text(ContentText::new("restored").unwrap())],
                extensions: ExtensionMap::default(),
            }],
            cumulative_usage: NormalizedUsage::unavailable(UsageMode::Cumulative),
            revision: Revision::new(3),
            plan: vec![],
            extensions: envelope("compat.agent-vesper"),
        }
    }

    fn envelope(namespace: &str) -> VersionedExtensionEnvelope {
        VersionedExtensionEnvelope {
            namespace: ExtensionNamespace::new(namespace).unwrap(),
            version: SchemaVersion::new(1).unwrap(),
            values: ExtensionMap::default(),
        }
    }

    fn metadata() -> SessionMetadata {
        SessionMetadata {
            session_id: SessionId::new("vesper-read-only").unwrap(),
            source: SessionSource::AgentVesper,
            byte_len: 0,
            modified: None,
            record_path: None,
            metadata_path: None,
            origin: crate::MetadataOrigin::JsonFallback,
            title: None,
            cwd: "/fixture".into(),
            updated_at: None,
            model: None,
            provider: None,
            parent_session_id: None,
            branch_root_id: None,
            safe_preview: None,
            read_only: true,
        }
    }

    fn decoder() -> VesperSessionDecoder {
        let provider = ProviderId::new("zai").unwrap();
        let model = QualifiedModelId {
            provider_id: provider.clone(),
            model_id: ModelId::new("glm-5.2").unwrap(),
        };
        VesperSessionDecoder::new(
            VesperDecodeBounds::default(),
            CompatibilityAvailability::default()
                .with_provider(provider.clone())
                .with_model(model)
                .with_endpoint(provider, EndpointId::new("zai-coding").unwrap()),
        )
    }

    #[test]
    fn future_format_decodes_read_only_without_known_field_loss() {
        let bytes = serde_json::to_vec(&record()).unwrap();
        let VesperLoadOutcome::Loaded(state) = decoder().decode_record(metadata(), &bytes) else {
            panic!("record failed to decode")
        };
        assert_eq!(state.session_id.as_str(), "vesper-read-only");
        assert_eq!(state.history.len(), 1);
        assert!(state.configuration_status.is_ready());
        assert!(matches!(
            state.compatibility,
            SessionCompatibilityData::AgentVesper(_)
        ));
    }

    #[test]
    fn future_format_rejects_versions_bounds_and_identity_mismatch() {
        let mut value = serde_json::to_value(record()).unwrap();
        value["version"] = serde_json::json!(2);
        assert!(matches!(
            decoder().decode_record(metadata(), &serde_json::to_vec(&value).unwrap()),
            VesperLoadOutcome::UnsupportedVersion(2)
        ));

        let bounds = VesperDecodeBounds {
            max_messages: 0,
            ..VesperDecodeBounds::default()
        };
        let provider = ProviderId::new("zai").unwrap();
        let bounded = VesperSessionDecoder::new(
            bounds,
            CompatibilityAvailability::default().with_provider(provider),
        );
        assert!(matches!(
            bounded.decode_record(metadata(), &serde_json::to_vec(&record()).unwrap()),
            VesperLoadOutcome::RejectedByBounds(_)
        ));

        let mut wrong = metadata();
        wrong.session_id = SessionId::new("other").unwrap();
        assert!(matches!(
            decoder().decode_record(wrong, &serde_json::to_vec(&record()).unwrap()),
            VesperLoadOutcome::Corrupt(_)
        ));

        let mut secret = serde_json::to_value(record()).unwrap();
        secret["provider_configuration"]["values"]["values"] =
            serde_json::json!({"provider:api-key": "raw-secret-canary"});
        assert!(matches!(
            decoder().decode_record(metadata(), &serde_json::to_vec(&secret).unwrap()),
            VesperLoadOutcome::Corrupt(_)
        ));
    }
}
