use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    CompatibilityRecordVersion, ModelId, ProviderId, ReasoningRetentionMode, SessionId,
    SessionLineage, SessionOperatingMode, SessionPermissionMode, SessionSnapshotHeader,
};

fn legacy_version() -> u32 {
    1
}
fn default_cwd() -> String {
    ".".into()
}
fn default_model() -> String {
    "glm-5.2".into()
}
fn default_thought_level() -> String {
    "enabled".into()
}
fn default_mode() -> String {
    "code".into()
}
fn default_endpoint() -> String {
    "coding".into()
}
fn default_profile() -> String {
    "balanced".into()
}
fn default_auxiliary_model() -> String {
    "main".into()
}
fn default_permission_mode() -> String {
    "ask".into()
}
fn default_mixture_mode() -> String {
    "off".into()
}

/// Frozen Native GLM ACP schema-1 record.
///
/// This DTO is deliberately compatibility-scoped: its GLM settings and flexible
/// nested state do not become provider-neutral session-domain fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacySessionV1 {
    /// Frozen schema version.
    #[serde(default = "legacy_version")]
    pub version: u32,
    /// Original workspace.
    #[serde(default = "default_cwd")]
    pub cwd: String,
    /// Legacy GLM model setting.
    #[serde(default = "default_model")]
    pub model: String,
    /// Legacy thought-level setting.
    #[serde(default = "default_thought_level")]
    pub thought_level: String,
    /// Code/plan mode.
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Legacy endpoint profile ID.
    #[serde(default = "default_endpoint")]
    pub api_endpoint: String,
    /// Legacy generation profile.
    #[serde(default = "default_profile")]
    pub generation_profile: String,
    /// Legacy auxiliary model.
    #[serde(default = "default_auxiliary_model")]
    pub auxiliary_model: String,
    /// Optional title.
    #[serde(default)]
    pub title: Option<String>,
    /// Parent session when forked.
    #[serde(default)]
    pub parent_session_id: Option<String>,
    /// Root session lineage.
    #[serde(default)]
    pub branch_root_id: Option<String>,
    /// Legacy permission mode.
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,
    /// Legacy plan records.
    #[serde(default)]
    pub plan: Vec<Value>,
    /// Ordered legacy messages, including provider-specific reasoning fields.
    #[serde(default)]
    pub messages: Vec<Value>,
    /// Cumulative input tokens.
    #[serde(default)]
    pub total_input_tokens: u64,
    /// Cumulative output tokens.
    #[serde(default)]
    pub total_output_tokens: u64,
    /// Cumulative cached tokens.
    #[serde(default)]
    pub total_cached_tokens: u64,
    /// Estimated current context use.
    #[serde(default)]
    pub estimated_tokens: u64,
    /// Legacy context-pressure level.
    #[serde(default)]
    pub context_pressure_level: u32,
    /// Bounded active task context.
    #[serde(default)]
    pub task_context: String,
    /// Compaction learning proposals.
    #[serde(default)]
    pub compaction_learning_proposals: Vec<String>,
    /// Compaction quality history.
    #[serde(default)]
    pub compaction_quality_history: Vec<Value>,
    /// Loaded instruction targets.
    #[serde(default)]
    pub instruction_targets: Vec<String>,
    /// Verification state envelope.
    #[serde(default)]
    pub verification: Value,
    /// Awareness state envelope.
    #[serde(default)]
    pub awareness: Value,
    /// Metacognition state envelope.
    #[serde(default)]
    pub metacognition: Value,
    /// Deliberation state envelope.
    #[serde(default)]
    pub deliberation: Value,
    /// Repository intelligence state envelope.
    #[serde(default)]
    pub repository_intelligence: Value,
    /// Meta-learning state envelope.
    #[serde(default)]
    pub meta_learning: Value,
    /// Persistent goal text.
    #[serde(default)]
    pub goal: String,
    /// Persistent subgoals.
    #[serde(default)]
    pub subgoals: Vec<String>,
    /// Goal pause state.
    #[serde(default)]
    pub goal_paused: bool,
    /// Goal turn budget use.
    #[serde(default)]
    pub goal_turns: u64,
    /// Legacy mixture mode.
    #[serde(default = "default_mixture_mode")]
    pub mixture_mode: String,
    /// Loaded tool names.
    #[serde(default)]
    pub loaded_tool_names: Vec<String>,
    /// Last checkpoint reference.
    #[serde(default)]
    pub last_checkpoint_id: String,
    /// Unrecognized frozen fields retained without interpretation.
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, Value>,
}

/// Compatibility decode/validation failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LegacySessionError {
    /// JSON was corrupt or not an object matching the compatibility DTO.
    #[error("legacy session JSON is malformed")]
    MalformedJson,
    /// Schema is not the frozen schema-1 contract.
    #[error("legacy session version {0} is unsupported")]
    UnsupportedVersion(u32),
    /// A compatibility-bounded value is invalid.
    #[error("legacy session field {field} exceeds its compatibility bound {maximum}")]
    BoundedValue {
        /// Field name.
        field: &'static str,
        /// Maximum length/count.
        maximum: usize,
    },
    /// A typed neutral conversion cannot represent one legacy value.
    #[error("legacy session field {field} has unsupported value {value}")]
    UnsupportedValue {
        /// Field name.
        field: &'static str,
        /// Exact legacy value.
        value: String,
    },
    /// A legacy identity is invalid.
    #[error("legacy session identity is invalid")]
    InvalidIdentity,
}

impl LegacySessionV1 {
    /// Decodes a frozen record without opening or writing a path.
    pub fn decode_json(bytes: &[u8]) -> Result<Self, LegacySessionError> {
        let record: Self =
            serde_json::from_slice(bytes).map_err(|_| LegacySessionError::MalformedJson)?;
        record.validate()?;
        Ok(record)
    }

    /// Encodes a record without storage side effects.
    pub fn encode_json(&self) -> Result<Vec<u8>, LegacySessionError> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|_| LegacySessionError::MalformedJson)
    }

    /// Validates source-compatible hard bounds without silently truncating.
    pub fn validate(&self) -> Result<(), LegacySessionError> {
        if CompatibilityRecordVersion::supported(self.version).is_err() {
            return Err(LegacySessionError::UnsupportedVersion(self.version));
        }
        check_len("task_context", self.task_context.len(), 2_000)?;
        check_len("goal", self.goal.len(), 4_000)?;
        check_len(
            "compaction_learning_proposals",
            self.compaction_learning_proposals.len(),
            50,
        )?;
        check_len("instruction_targets", self.instruction_targets.len(), 100)?;
        check_len("subgoals", self.subgoals.len(), 50)?;
        check_len("loaded_tool_names", self.loaded_tool_names.len(), 100)?;
        for value in &self.subgoals {
            check_len("subgoal", value.len(), 1_000)?;
        }
        Ok(())
    }

    /// Returns GLM-owned compatibility settings without promoting them into core fields.
    #[must_use]
    pub fn glm_settings(&self) -> LegacyGlmSettings<'_> {
        LegacyGlmSettings {
            model: &self.model,
            thought_level: &self.thought_level,
            api_endpoint: &self.api_endpoint,
            generation_profile: &self.generation_profile,
            auxiliary_model: &self.auxiliary_model,
        }
    }

    /// Returns whether persisted provider-visible reasoning is present.
    #[must_use]
    pub fn contains_persisted_reasoning(&self) -> bool {
        self.messages.iter().any(|message| {
            message
                .as_object()
                .is_some_and(|fields| fields.contains_key("reasoning_content"))
        })
    }

    /// Performs an explicit, fallible conversion of shared session-header fields.
    pub fn to_neutral_header(
        &self,
        session_id: SessionId,
    ) -> Result<SessionSnapshotHeader, LegacySessionError> {
        let parent_session_id = self
            .parent_session_id
            .as_deref()
            .map(SessionId::new)
            .transpose()
            .map_err(|_| LegacySessionError::InvalidIdentity)?;
        let root_session_id = SessionId::new(
            self.branch_root_id
                .as_deref()
                .unwrap_or_else(|| session_id.as_str()),
        )
        .map_err(|_| LegacySessionError::InvalidIdentity)?;
        let permission_mode = match self.permission_mode.as_str() {
            "ask" => SessionPermissionMode::Ask,
            "bypass" => SessionPermissionMode::Bypass,
            "read-only" => SessionPermissionMode::ReadOnly,
            value => {
                return Err(LegacySessionError::UnsupportedValue {
                    field: "permission_mode",
                    value: value.into(),
                });
            }
        };
        let operating_mode = match self.mode.as_str() {
            "code" => SessionOperatingMode::Code,
            "plan" => SessionOperatingMode::Plan,
            value => {
                return Err(LegacySessionError::UnsupportedValue {
                    field: "mode",
                    value: value.into(),
                });
            }
        };
        Ok(SessionSnapshotHeader {
            session_id,
            lineage: SessionLineage {
                root_session_id,
                parent_session_id,
            },
            provider_id: ProviderId::new("glm").map_err(|_| LegacySessionError::InvalidIdentity)?,
            model_id: ModelId::new(self.model.clone())
                .map_err(|_| LegacySessionError::InvalidIdentity)?,
            permission_mode,
            operating_mode,
            reasoning_retention: ReasoningRetentionMode::Persist,
        })
    }
}

fn check_len(field: &'static str, actual: usize, maximum: usize) -> Result<(), LegacySessionError> {
    if actual > maximum {
        Err(LegacySessionError::BoundedValue { field, maximum })
    } else {
        Ok(())
    }
}

/// Borrowed GLM-only settings from a legacy compatibility record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyGlmSettings<'a> {
    /// Legacy model.
    pub model: &'a str,
    /// Legacy thought level.
    pub thought_level: &'a str,
    /// Legacy endpoint key.
    pub api_endpoint: &'a str,
    /// Legacy generation profile.
    pub generation_profile: &'a str,
    /// Legacy auxiliary model.
    pub auxiliary_model: &'a str,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn omitted_fields_receive_documented_defaults() {
        let record = LegacySessionV1::decode_json(br#"{"cwd":"/workspace"}"#).unwrap();
        assert_eq!(record.version, 1);
        assert_eq!(record.model, "glm-5.2");
        assert_eq!(record.permission_mode, "ask");
        assert_eq!(record.api_endpoint, "coding");
    }

    #[test]
    fn unknown_fields_survive_round_trip() {
        let record =
            LegacySessionV1::decode_json(br#"{"future_field":{"preserve":true}}"#).unwrap();
        let encoded = record.encode_json().unwrap();
        let decoded = LegacySessionV1::decode_json(&encoded).unwrap();
        assert_eq!(
            decoded.unknown_fields.get("future_field"),
            Some(&json!({"preserve": true}))
        );
    }

    #[test]
    fn corrupt_and_invalid_bounded_values_are_explicit() {
        assert_eq!(
            LegacySessionV1::decode_json(b"{broken"),
            Err(LegacySessionError::MalformedJson)
        );
        let oversized = format!(r#"{{"task_context":"{}"}}"#, "x".repeat(2_001));
        assert!(matches!(
            LegacySessionV1::decode_json(oversized.as_bytes()),
            Err(LegacySessionError::BoundedValue {
                field: "task_context",
                ..
            })
        ));
    }
}
