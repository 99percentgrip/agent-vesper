use vesper_domain::{
    ContentPart, ConversationMessage, EndpointId, NormalizedUsage, ProviderId, QualifiedModelId,
    Revision, SessionId, SessionLineage, SessionOperatingMode, SessionPermissionMode, TurnId,
    UsageMode, WorkspaceRoot,
};
use vesper_provider::{ProviderConfiguration, ReasoningIntent};
use vesper_sessions::{
    PersistedSessionState, ReplayPlan, SessionCompatibilityData, SessionConfigurationStatus,
    SessionSource,
};

/// Immutable snapshot of one ephemeral session.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSnapshot {
    /// Identity.
    pub session_id: SessionId,
    /// Lineage.
    pub lineage: SessionLineage,
    /// Workspace roots supplied by the client.
    pub workspace_roots: Vec<WorkspaceRoot>,
    /// Selected provider.
    pub provider_id: ProviderId,
    /// Selected model.
    pub model: QualifiedModelId,
    /// Selected endpoint reference.
    pub endpoint_id: Option<EndpointId>,
    /// Provider-owned configuration.
    pub provider_configuration: ProviderConfiguration,
    /// Read source; ephemeral sessions originate in memory.
    pub source: SessionSource,
    /// Whether a new provider turn can be dispatched.
    pub configuration_status: SessionConfigurationStatus,
    /// Operating mode.
    pub operating_mode: SessionOperatingMode,
    /// Permission mode retained for future policy integration.
    pub permission_mode: SessionPermissionMode,
    /// Accepted conversation history.
    pub history: Vec<ConversationMessage>,
    /// Latest cumulative usage.
    pub cumulative_usage: NormalizedUsage,
    /// State revision.
    pub revision: Revision,
    /// Active turn, if any.
    pub active_turn: Option<TurnId>,
    /// Closed state.
    pub closed: bool,
    /// Persisted replay plan, absent for ordinary in-memory sessions.
    pub replay: Option<ReplayPlan>,
    /// Frozen record retained for a future explicit compatibility writer.
    pub compatibility: Option<SessionCompatibilityData>,
    /// Session-scoped provider reasoning override (e.g. the GLM
    /// `thought_level` dial). When `None`, turns fall back to the runtime
    /// default reasoning. Set by the `UpdateSessionReasoning` command and
    /// applied to every subsequent turn's `ProviderRequest.reasoning`.
    pub reasoning: Option<ReasoningIntent>,
}

impl SessionSnapshot {
    /// Converts pure read-only compatibility state into runtime-owned state.
    #[must_use]
    pub fn from_persisted(value: PersistedSessionState) -> Self {
        Self {
            session_id: value.session_id,
            lineage: value.lineage,
            workspace_roots: value.workspace_roots,
            provider_id: value.provider_id.clone(),
            model: value.model,
            endpoint_id: Some(value.endpoint_id),
            provider_configuration: ProviderConfiguration {
                provider_id: value.provider_configuration.provider_id,
                values: value.provider_configuration.values,
            },
            source: value.source,
            configuration_status: value.configuration_status,
            operating_mode: value.operating_mode,
            permission_mode: value.permission_mode,
            history: value.history,
            cumulative_usage: value.cumulative_usage,
            revision: value.revision,
            active_turn: None,
            closed: false,
            replay: Some(value.replay),
            compatibility: Some(value.compatibility),
            // Persisted state does not yet carry a reasoning-mode seed
            // (ADR 0009); turns fall back to the runtime default until the
            // persistence layer gains the field.
            reasoning: None,
        }
    }

    pub(crate) fn fork(&self, child: SessionId) -> Self {
        let root = self.lineage.root_session_id.clone();
        Self {
            session_id: child,
            lineage: SessionLineage {
                root_session_id: root,
                parent_session_id: Some(self.session_id.clone()),
            },
            workspace_roots: self.workspace_roots.clone(),
            provider_id: self.provider_id.clone(),
            model: self.model.clone(),
            endpoint_id: self.endpoint_id.clone(),
            provider_configuration: self.provider_configuration.clone(),
            source: self.source.clone(),
            configuration_status: self.configuration_status.clone(),
            operating_mode: self.operating_mode,
            permission_mode: self.permission_mode,
            history: self.history.clone(),
            cumulative_usage: self.cumulative_usage.clone(),
            revision: Revision::new(0),
            active_turn: None,
            closed: false,
            replay: self.replay.clone(),
            compatibility: self.compatibility.clone(),
            reasoning: self.reasoning.clone(),
        }
    }

    pub(crate) fn initial(
        session_id: SessionId,
        workspace_roots: Vec<WorkspaceRoot>,
        provider_id: ProviderId,
        model: QualifiedModelId,
        provider_configuration: ProviderConfiguration,
        endpoint: EndpointId,
        reasoning: Option<ReasoningIntent>,
    ) -> Self {
        Self {
            lineage: SessionLineage {
                root_session_id: session_id.clone(),
                parent_session_id: None,
            },
            session_id,
            workspace_roots,
            provider_id,
            model,
            endpoint_id: Some(endpoint),
            provider_configuration,
            source: SessionSource::InMemory,
            configuration_status: SessionConfigurationStatus::Ready,
            operating_mode: SessionOperatingMode::Code,
            permission_mode: SessionPermissionMode::Ask,
            history: Vec::new(),
            cumulative_usage: NormalizedUsage::unavailable(UsageMode::Cumulative),
            revision: Revision::new(0),
            active_turn: None,
            closed: false,
            replay: None,
            compatibility: None,
            reasoning,
        }
    }
}

/// Completion information returned to a correlated adapter prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionTurnResult {
    /// Turn identity.
    pub turn_id: TurnId,
    /// User message identity.
    pub user_message_id: vesper_domain::MessageId,
    /// Terminal outcome.
    pub outcome: vesper_domain::FinishOutcome,
    /// Whether visible output escaped.
    pub visible_output_emitted: bool,
    /// Assistant-visible content accepted into history.
    pub assistant_content: Vec<ContentPart>,
    /// Latest provider usage accepted for the session.
    pub usage: Option<NormalizedUsage>,
}
