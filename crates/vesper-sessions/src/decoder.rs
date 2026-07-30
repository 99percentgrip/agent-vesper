use serde_json::Value;
use vesper_domain::{LegacySessionError, LegacySessionV1, SessionId};

use crate::{SessionMetadata, SessionReader, SessionStoreError};

/// Hard limits applied before a legacy record is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyDecodeBounds {
    pub max_file_bytes: usize,
    pub max_messages: usize,
    pub max_content_bytes: usize,
    pub max_plan_items: usize,
    pub max_plan_bytes: usize,
    pub max_roots: usize,
    pub max_root_bytes: usize,
    pub max_metadata_extension_fields: usize,
    pub max_unknown_bytes: usize,
    pub max_unknown_nodes: usize,
    pub max_json_depth: usize,
    pub max_lineage_id_bytes: usize,
    pub max_compatibility_array_items: usize,
    pub max_compatibility_value_bytes: usize,
}

impl Default for LegacyDecodeBounds {
    fn default() -> Self {
        Self {
            max_file_bytes: 16 * 1024 * 1024,
            max_messages: 10_000,
            max_content_bytes: 1024 * 1024,
            max_plan_items: 1_000,
            max_plan_bytes: 1024 * 1024,
            max_roots: 128,
            max_root_bytes: 4_096,
            max_metadata_extension_fields: 256,
            max_unknown_bytes: 1024 * 1024,
            max_unknown_nodes: 100_000,
            max_json_depth: 64,
            max_lineage_id_bytes: 256,
            max_compatibility_array_items: 1_000,
            max_compatibility_value_bytes: 4 * 1024 * 1024,
        }
    }
}

/// A record exceeded a named compatibility limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundViolation {
    pub field: &'static str,
    pub maximum: usize,
}

/// Safe corrupt-record classification without record contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorruptLegacyRecord {
    MalformedJson,
    InvalidShape,
    CompatibilityValue,
    Unreadable,
}

/// Successfully decoded compatibility record with its read-only provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedLegacySession {
    pub metadata: SessionMetadata,
    pub session: LegacySessionV1,
}

/// Typed result of a bounded legacy load.
#[derive(Debug, Clone, PartialEq)]
pub enum LegacyLoadOutcome {
    Loaded(Box<DecodedLegacySession>),
    Missing,
    Corrupt(CorruptLegacyRecord),
    UnsupportedVersion(u32),
    RejectedByBounds(BoundViolation),
    PermissionDenied,
    UnsafePath,
}

/// Stateless bounded decoder for frozen schema-v1 records.
#[derive(Debug, Clone, Copy)]
pub struct LegacySessionDecoder {
    bounds: LegacyDecodeBounds,
}

impl LegacySessionDecoder {
    #[must_use]
    pub const fn new(bounds: LegacyDecodeBounds) -> Self {
        Self { bounds }
    }

    #[must_use]
    pub const fn bounds(self) -> LegacyDecodeBounds {
        self.bounds
    }

    /// Loads raw bytes through a read-only port and classifies every outcome.
    pub async fn load(
        &self,
        reader: &dyn SessionReader,
        session_id: &SessionId,
    ) -> LegacyLoadOutcome {
        match reader.load(session_id).await {
            Ok(Some(record)) => self.decode_record(record.metadata, &record.bytes),
            Ok(None) => LegacyLoadOutcome::Missing,
            Err(error) => self.classify_store_error(error),
        }
    }

    /// Decodes already bounded raw bytes without filesystem side effects.
    #[must_use]
    pub fn decode_record(&self, metadata: SessionMetadata, bytes: &[u8]) -> LegacyLoadOutcome {
        if bytes.len() > self.bounds.max_file_bytes {
            return rejected("file_bytes", self.bounds.max_file_bytes);
        }
        let value: Value = match serde_json::from_slice(bytes) {
            Ok(value) => value,
            Err(_) => {
                return LegacyLoadOutcome::Corrupt(CorruptLegacyRecord::MalformedJson);
            }
        };
        let Some(object) = value.as_object() else {
            return LegacyLoadOutcome::Corrupt(CorruptLegacyRecord::InvalidShape);
        };
        let version = match object.get("version") {
            None => 1,
            Some(Value::Number(number)) => {
                match number.as_u64().and_then(|value| u32::try_from(value).ok()) {
                    Some(version) => version,
                    None => return LegacyLoadOutcome::Corrupt(CorruptLegacyRecord::InvalidShape),
                }
            }
            Some(_) => return LegacyLoadOutcome::Corrupt(CorruptLegacyRecord::InvalidShape),
        };
        if version != 1 {
            return LegacyLoadOutcome::UnsupportedVersion(version);
        }

        let session: LegacySessionV1 = match serde_json::from_value(value) {
            Ok(session) => session,
            Err(_) => return LegacyLoadOutcome::Corrupt(CorruptLegacyRecord::InvalidShape),
        };
        if let Err(outcome) = self.validate(&session) {
            return outcome;
        }
        match session.validate() {
            Ok(()) => {
                LegacyLoadOutcome::Loaded(Box::new(DecodedLegacySession { metadata, session }))
            }
            Err(LegacySessionError::UnsupportedVersion(version)) => {
                LegacyLoadOutcome::UnsupportedVersion(version)
            }
            Err(LegacySessionError::BoundedValue { field, maximum }) => {
                LegacyLoadOutcome::RejectedByBounds(BoundViolation { field, maximum })
            }
            Err(LegacySessionError::MalformedJson | LegacySessionError::InvalidIdentity) => {
                LegacyLoadOutcome::Corrupt(CorruptLegacyRecord::InvalidShape)
            }
            Err(LegacySessionError::UnsupportedValue { .. }) => {
                LegacyLoadOutcome::Corrupt(CorruptLegacyRecord::CompatibilityValue)
            }
        }
    }

    fn validate(&self, session: &LegacySessionV1) -> Result<(), LegacyLoadOutcome> {
        ensure_len("messages", session.messages.len(), self.bounds.max_messages)?;
        ensure_len("plan", session.plan.len(), self.bounds.max_plan_items)?;
        ensure_len("cwd", session.cwd.len(), self.bounds.max_root_bytes)?;
        for (field, value, maximum) in [
            ("model", session.model.as_str(), 256),
            ("thought_level", session.thought_level.as_str(), 256),
            ("mode", session.mode.as_str(), 256),
            ("api_endpoint", session.api_endpoint.as_str(), 1_024),
            (
                "generation_profile",
                session.generation_profile.as_str(),
                256,
            ),
            ("auxiliary_model", session.auxiliary_model.as_str(), 256),
            ("permission_mode", session.permission_mode.as_str(), 256),
            ("mixture_mode", session.mixture_mode.as_str(), 256),
            (
                "last_checkpoint_id",
                session.last_checkpoint_id.as_str(),
                256,
            ),
        ] {
            ensure_len(field, value.len(), maximum)?;
        }
        ensure_optional_len("title", session.title.as_deref(), 1_024)?;
        ensure_optional_len(
            "parent_session_id",
            session.parent_session_id.as_deref(),
            self.bounds.max_lineage_id_bytes,
        )?;
        ensure_optional_len(
            "branch_root_id",
            session.branch_root_id.as_deref(),
            self.bounds.max_lineage_id_bytes,
        )?;
        for identity in [
            session.parent_session_id.as_deref(),
            session.branch_root_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if SessionId::new(identity).is_err() {
                return Err(LegacyLoadOutcome::Corrupt(
                    CorruptLegacyRecord::CompatibilityValue,
                ));
            }
        }

        let plan_stats = measure_values(
            session.plan.iter(),
            "plan",
            self.bounds.max_json_depth,
            self.bounds.max_unknown_nodes,
        )?;
        ensure_len("plan_bytes", plan_stats.bytes, self.bounds.max_plan_bytes)?;

        for message in &session.messages {
            validate_content_fields(message, self.bounds)?;
        }

        for (field, actual) in [
            (
                "compaction_quality_history",
                session.compaction_quality_history.len(),
            ),
            (
                "compaction_learning_proposals",
                session.compaction_learning_proposals.len(),
            ),
            ("instruction_targets", session.instruction_targets.len()),
            ("subgoals", session.subgoals.len()),
            ("loaded_tool_names", session.loaded_tool_names.len()),
        ] {
            ensure_len(field, actual, self.bounds.max_compatibility_array_items)?;
        }
        for tool in &session.loaded_tool_names {
            ensure_len("loaded_tool_name", tool.len(), 256)?;
        }
        for target in &session.instruction_targets {
            ensure_len(
                "instruction_target",
                target.len(),
                self.bounds.max_root_bytes,
            )?;
        }
        for proposal in &session.compaction_learning_proposals {
            ensure_len("compaction_learning_proposal", proposal.len(), 1_000)?;
        }

        let compatibility_stats = measure_values(
            [
                &session.verification,
                &session.awareness,
                &session.metacognition,
                &session.deliberation,
                &session.repository_intelligence,
                &session.meta_learning,
            ]
            .into_iter()
            .chain(session.compaction_quality_history.iter()),
            "compatibility_values",
            self.bounds.max_json_depth,
            self.bounds.max_unknown_nodes,
        )?;
        ensure_len(
            "compatibility_value_bytes",
            compatibility_stats.bytes,
            self.bounds.max_compatibility_value_bytes,
        )?;

        ensure_len(
            "metadata_extensions",
            session.unknown_fields.len(),
            self.bounds.max_metadata_extension_fields,
        )?;
        let mut unknown_stats = measure_values(
            session.unknown_fields.values(),
            "unknown_fields",
            self.bounds.max_json_depth,
            self.bounds.max_unknown_nodes,
        )?;
        for key in session.unknown_fields.keys() {
            unknown_stats.bytes = unknown_stats.bytes.checked_add(key.len()).ok_or_else(|| {
                rejected_value("unknown_field_bytes", self.bounds.max_unknown_bytes)
            })?;
        }
        ensure_len(
            "unknown_field_bytes",
            unknown_stats.bytes,
            self.bounds.max_unknown_bytes,
        )?;
        validate_additional_roots(session, self.bounds)?;
        Ok(())
    }

    fn classify_store_error(&self, error: SessionStoreError) -> LegacyLoadOutcome {
        match error {
            SessionStoreError::Io(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                LegacyLoadOutcome::PermissionDenied
            }
            SessionStoreError::PathEscapesRoot
            | SessionStoreError::RootNotAbsolute
            | SessionStoreError::InvalidFileName(_) => LegacyLoadOutcome::UnsafePath,
            SessionStoreError::RecordLimitExceeded { maximum } => {
                LegacyLoadOutcome::RejectedByBounds(BoundViolation {
                    field: "file_bytes",
                    maximum: usize::try_from(maximum).unwrap_or(usize::MAX),
                })
            }
            _ => LegacyLoadOutcome::Corrupt(CorruptLegacyRecord::Unreadable),
        }
    }
}

impl Default for LegacySessionDecoder {
    fn default() -> Self {
        Self::new(LegacyDecodeBounds::default())
    }
}

fn validate_additional_roots(
    session: &LegacySessionV1,
    bounds: LegacyDecodeBounds,
) -> Result<(), LegacyLoadOutcome> {
    for field in [
        "additional_directories",
        "additional_roots",
        "additional_dirs",
    ] {
        let Some(value) = session.unknown_fields.get(field) else {
            continue;
        };
        let Some(roots) = value.as_array() else {
            return Err(LegacyLoadOutcome::Corrupt(
                CorruptLegacyRecord::CompatibilityValue,
            ));
        };
        ensure_len("additional_roots", roots.len(), bounds.max_roots)?;
        for root in roots {
            let Some(root) = root.as_str() else {
                return Err(LegacyLoadOutcome::Corrupt(
                    CorruptLegacyRecord::CompatibilityValue,
                ));
            };
            ensure_len("additional_root", root.len(), bounds.max_root_bytes)?;
        }
    }
    Ok(())
}

fn validate_content_fields(
    value: &Value,
    bounds: LegacyDecodeBounds,
) -> Result<(), LegacyLoadOutcome> {
    let mut stack = vec![(value, 0_usize)];
    let mut nodes = 0_usize;
    while let Some((current, depth)) = stack.pop() {
        nodes = nodes.saturating_add(1);
        if nodes > bounds.max_unknown_nodes || depth > bounds.max_json_depth {
            return Err(rejected_value(
                "message_structure",
                bounds.max_unknown_nodes,
            ));
        }
        match current {
            Value::Object(fields) => {
                for (key, child) in fields {
                    if matches!(key.as_str(), "content" | "reasoning_content") {
                        let stats = measure_values(
                            std::iter::once(child),
                            "message_content",
                            bounds.max_json_depth,
                            bounds.max_unknown_nodes,
                        )?;
                        ensure_len("message_content", stats.bytes, bounds.max_content_bytes)?;
                    }
                    stack.push((child, depth + 1));
                }
            }
            Value::Array(values) => {
                stack.extend(values.iter().map(|child| (child, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ValueStats {
    bytes: usize,
}

fn measure_values<'a>(
    values: impl Iterator<Item = &'a Value>,
    field: &'static str,
    max_depth: usize,
    max_nodes: usize,
) -> Result<ValueStats, LegacyLoadOutcome> {
    let mut stack = values.map(|value| (value, 0_usize)).collect::<Vec<_>>();
    let mut bytes = 0_usize;
    let mut nodes = 0_usize;
    while let Some((value, depth)) = stack.pop() {
        nodes = nodes.saturating_add(1);
        if nodes > max_nodes || depth > max_depth {
            return Err(rejected_value(field, max_nodes));
        }
        bytes = bytes
            .checked_add(match value {
                Value::Null => 4,
                Value::Bool(_) => 5,
                Value::Number(number) => number.to_string().len(),
                Value::String(value) => value.len(),
                Value::Array(values) => {
                    stack.extend(values.iter().map(|child| (child, depth + 1)));
                    0
                }
                Value::Object(values) => {
                    for (key, child) in values {
                        bytes = bytes
                            .checked_add(key.len())
                            .ok_or_else(|| rejected_value(field, max_nodes))?;
                        stack.push((child, depth + 1));
                    }
                    0
                }
            })
            .ok_or_else(|| rejected_value(field, max_nodes))?;
    }
    Ok(ValueStats { bytes })
}

fn ensure_optional_len(
    field: &'static str,
    value: Option<&str>,
    maximum: usize,
) -> Result<(), LegacyLoadOutcome> {
    if let Some(value) = value {
        ensure_len(field, value.len(), maximum)?;
    }
    Ok(())
}

fn ensure_len(field: &'static str, actual: usize, maximum: usize) -> Result<(), LegacyLoadOutcome> {
    if actual > maximum {
        Err(rejected_value(field, maximum))
    } else {
        Ok(())
    }
}

fn rejected(field: &'static str, maximum: usize) -> LegacyLoadOutcome {
    LegacyLoadOutcome::RejectedByBounds(BoundViolation { field, maximum })
}

fn rejected_value(field: &'static str, maximum: usize) -> LegacyLoadOutcome {
    rejected(field, maximum)
}
