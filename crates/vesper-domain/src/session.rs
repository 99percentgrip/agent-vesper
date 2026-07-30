use serde::{Deserialize, Serialize};

use crate::{ModelId, ProviderId, SessionId, SessionOperatingMode, SessionPermissionMode};

/// New-store reasoning retention modes. Initial GLM parity uses `Persist`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningRetentionMode {
    /// Persist eligible provider-visible and opaque continuation records.
    #[default]
    Persist,
    /// Retain only for the active process/session.
    SessionOnly,
    /// Do not retain.
    Disabled,
}

/// Session ancestry preserved across resume and fork.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLineage {
    /// Root session.
    pub root_session_id: SessionId,
    /// Immediate parent when forked.
    pub parent_session_id: Option<SessionId>,
}

/// Stable non-persistence-specific session header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshotHeader {
    /// Session identity.
    pub session_id: SessionId,
    /// Lineage.
    pub lineage: SessionLineage,
    /// Active provider.
    pub provider_id: ProviderId,
    /// Active provider-qualified model.
    pub model_id: ModelId,
    /// Permission mode.
    pub permission_mode: SessionPermissionMode,
    /// Code versus plan operation mode.
    pub operating_mode: SessionOperatingMode,
    /// Reasoning retention mode.
    pub reasoning_retention: ReasoningRetentionMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_retention_default_and_all_modes_round_trip() {
        assert_eq!(
            ReasoningRetentionMode::default(),
            ReasoningRetentionMode::Persist
        );
        for mode in [
            ReasoningRetentionMode::Persist,
            ReasoningRetentionMode::SessionOnly,
            ReasoningRetentionMode::Disabled,
        ] {
            let encoded = serde_json::to_string(&mode).unwrap();
            let decoded: ReasoningRetentionMode = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, mode);
        }
    }
}
