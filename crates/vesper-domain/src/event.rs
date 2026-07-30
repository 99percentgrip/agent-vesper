use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    BoundedString, ContentPart, ContentText, CorrelationId, ErrorInfo, EventId, EventSchemaVersion,
    ExtensionMap, FinishOutcome, Goal, MessageId, NormalizedUsage, PermissionOutcome,
    PermissionRequestId, Plan, ProviderToolName, ReasoningKind, ReasoningRetention, Revision,
    RuntimeAuthenticationMethod, RuntimeCapability, SafeMessage, SessionId, ToolCall, ToolCallId,
    ToolExecutionClass, ToolId, ToolResult, TurnId,
};
use std::collections::BTreeSet;

/// Monotonic event sequence scoped to a runtime, session, or turn stream.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct EventSequence(u64);

impl EventSequence {
    /// Creates a sequence.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric sequence.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next sequence if it does not overflow.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Provider-neutral events emitted by the future harness runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "kebab-case")]
pub enum HarnessEventPayload {
    /// Runtime initialization and negotiated capabilities completed.
    RuntimeInitialized {
        /// Negotiated runtime capabilities.
        capabilities: BTreeSet<RuntimeCapability>,
        /// Available authentication methods.
        authentication_methods: Vec<RuntimeAuthenticationMethod>,
        /// Safe adapter metadata.
        metadata: ExtensionMap,
    },
    /// Session was created.
    SessionCreated {
        /// State revision.
        revision: Revision,
        /// Safe metadata.
        metadata: ExtensionMap,
    },
    /// Existing session state was loaded.
    SessionLoaded {
        /// State revision.
        revision: Revision,
        /// Replay event count available.
        replay_event_count: u64,
    },
    /// Result of one session-list command.
    SessionListProduced {
        /// Ordered summaries.
        sessions: Vec<SessionSummary>,
    },
    /// Session metadata changed.
    SessionMetadataChanged {
        /// New revision.
        revision: Revision,
        /// Changed namespaced metadata.
        changes: ExtensionMap,
    },
    /// User message was accepted and assigned to a turn.
    UserMessageAccepted {
        /// Stable user-message identity.
        message_id: MessageId,
    },
    /// Provider response has started.
    ResponseStarted {
        /// Redacted/provider-namespaced response metadata.
        metadata: ExtensionMap,
    },
    /// Exposed reasoning delta.
    ReasoningDelta {
        /// Stream-local identifier.
        stream_id: BoundedString<128>,
        /// Text or opaque reference.
        text: ContentText,
        /// Reasoning classification.
        kind: ReasoningKind,
        /// Retention instruction.
        retention: ReasoningRetention,
    },
    /// User-visible content delta.
    ContentDelta {
        /// Stream-local identifier.
        stream_id: BoundedString<128>,
        /// Ordered content part.
        part: ContentPart,
    },
    /// Tool call began.
    ToolCallStarted {
        /// Provider stream index.
        index: u32,
        /// Optional stable call ID before assembly completes.
        call_id: Option<ToolCallId>,
        /// Optional tool name before assembly completes.
        name: Option<ProviderToolName>,
    },
    /// Incremental tool argument/name/ID fragment.
    ToolCallUpdated {
        /// Provider stream index.
        index: u32,
        /// Optional ID fragment.
        id_fragment: Option<BoundedString<256>>,
        /// Optional name fragment.
        name_fragment: Option<BoundedString<128>>,
        /// Arguments fragment.
        arguments_fragment: ContentText,
    },
    /// Fully assembled tool call.
    ToolCallCompleted(ToolCall),
    /// Tool execution returned a linked result.
    ToolResultCompleted(ToolResult),
    /// Tool execution failed before a normal result.
    ToolCallFailed {
        /// Linked call.
        call_id: ToolCallId,
        /// Safe classified error.
        error: ErrorInfo,
    },
    /// Normalized usage update.
    UsageUpdated(NormalizedUsage),
    /// Permission interaction started.
    PermissionRequested {
        /// Request ID.
        request_id: PermissionRequestId,
        /// Safe summary.
        summary: SafeMessage,
        /// Requested authority class.
        operation_class: ToolExecutionClass,
        /// Tool identity where the request originated from a tool.
        tool_id: Option<ToolId>,
    },
    /// Permission interaction completed.
    PermissionResolved {
        /// Request ID.
        request_id: PermissionRequestId,
        /// Decision.
        outcome: PermissionOutcome,
    },
    /// Plan state changed.
    PlanChanged(Plan),
    /// Goal state changed.
    GoalChanged(Goal),
    /// Provider availability/rate/quota status.
    ProviderStatusUpdated {
        /// Namespaced safe provider status.
        status: ExtensionMap,
    },
    /// Context pressure changed.
    ContextPressureUpdated {
        /// Current used units where known.
        used: Option<u64>,
        /// Current capacity where known.
        capacity: Option<u64>,
        /// Bounded level selected by the future runtime.
        level: u8,
    },
    /// Capability fallback was selected and must be observable.
    FallbackApplied {
        /// Capability name.
        capability: BoundedString<128>,
        /// Safe fallback description.
        fallback: SafeMessage,
    },
    /// Non-terminal warning.
    Warning {
        /// Safe warning.
        message: SafeMessage,
    },
    /// Recoverable error that does not terminate the turn.
    RecoverableError(ErrorInfo),
    /// Exactly one normal terminal turn outcome.
    TurnCompleted {
        /// Finish classification.
        outcome: FinishOutcome,
        /// Provider metadata.
        metadata: ExtensionMap,
    },
    /// Explicit terminal cancellation.
    TurnCancelled {
        /// Whether any visible output preceded cancellation.
        visible_output_emitted: bool,
    },
    /// Session closed.
    SessionClosed,
    /// Runtime stopped cleanly.
    RuntimeShutdown,
}

/// Minimal session listing entry; persistence-specific indexes remain external.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Session identity.
    pub session_id: SessionId,
    /// Optional safe title.
    pub title: Option<BoundedString<512>>,
    /// Current state revision.
    pub revision: Revision,
    /// Whether the session is closed.
    pub closed: bool,
    /// Safe extensible listing metadata.
    #[serde(default)]
    pub metadata: ExtensionMap,
}

impl HarnessEventPayload {
    /// Whether this payload is terminal for a turn.
    #[must_use]
    pub const fn is_turn_terminal(&self) -> bool {
        matches!(
            self,
            Self::TurnCompleted { .. } | Self::TurnCancelled { .. }
        )
    }

    /// Whether this payload exposes user-visible output.
    #[must_use]
    pub const fn is_visible_output(&self) -> bool {
        matches!(
            self,
            Self::ContentDelta { .. }
                | Self::ReasoningDelta {
                    kind: ReasoningKind::ProviderVisible | ReasoningKind::Summary,
                    ..
                }
                | Self::ToolCallStarted { .. }
                | Self::ToolCallUpdated { .. }
                | Self::ToolCallCompleted(_)
                | Self::ToolResultCompleted(_)
        )
    }
}

/// Sequenced event envelope consumed by future ACP/CLI/TUI adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessEvent {
    /// Event schema version.
    pub schema_version: EventSchemaVersion,
    /// Stable event identity.
    pub event_id: EventId,
    /// Correlation with the initiating command/request.
    pub correlation_id: Option<CorrelationId>,
    /// Session identity, absent only for runtime-scope events.
    pub session_id: Option<SessionId>,
    /// Turn identity, present for turn-scope events.
    pub turn_id: Option<TurnId>,
    /// Monotonic sequence within the derived scope.
    pub sequence: EventSequence,
    /// Event payload.
    pub payload: HarnessEventPayload,
}

/// Stable event sequence ownership key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum EventScope {
    Runtime,
    Session(SessionId),
    Turn(SessionId, TurnId),
}

impl HarnessEvent {
    fn scope(&self) -> Result<EventScope, EventSequenceError> {
        match (&self.session_id, &self.turn_id) {
            (None, None) => Ok(EventScope::Runtime),
            (Some(session), None) => Ok(EventScope::Session(session.clone())),
            (Some(session), Some(turn)) => Ok(EventScope::Turn(session.clone(), turn.clone())),
            (None, Some(_)) => Err(EventSequenceError::TurnWithoutSession),
        }
    }
}

/// Sequence validation failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EventSequenceError {
    /// Sequence was not the next value in its ownership scope.
    #[error("expected event sequence {expected}, received {actual}")]
    NonMonotonic {
        /// Expected sequence.
        expected: u64,
        /// Actual sequence.
        actual: u64,
    },
    /// Turn identity cannot exist without session ownership.
    #[error("turn-scoped event is missing its session identity")]
    TurnWithoutSession,
    /// More than one terminal event was emitted for one turn.
    #[error("turn emitted more than one terminal event")]
    DuplicateTurnTerminal,
    /// Any turn event after terminal is invalid.
    #[error("turn emitted an event after its terminal event")]
    EventAfterTurnTerminal,
}

/// Ordered event collection enforcing per-scope monotonicity and terminal uniqueness.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventLog {
    events: Vec<HarnessEvent>,
    next_by_scope: BTreeMap<EventScope, u64>,
    terminal_by_turn: BTreeMap<(SessionId, TurnId), bool>,
}

impl EventLog {
    /// Appends the next event.
    pub fn push(&mut self, event: HarnessEvent) -> Result<(), EventSequenceError> {
        let scope = event.scope()?;
        let expected = self.next_by_scope.get(&scope).copied().unwrap_or(0);
        if event.sequence.get() != expected {
            return Err(EventSequenceError::NonMonotonic {
                expected,
                actual: event.sequence.get(),
            });
        }
        if let (Some(session), Some(turn)) = (&event.session_id, &event.turn_id)
            && self
                .terminal_by_turn
                .contains_key(&(session.clone(), turn.clone()))
        {
            return if event.payload.is_turn_terminal() {
                Err(EventSequenceError::DuplicateTurnTerminal)
            } else {
                Err(EventSequenceError::EventAfterTurnTerminal)
            };
        }
        if event.payload.is_turn_terminal() {
            let Some(session) = event.session_id.clone() else {
                return Err(EventSequenceError::TurnWithoutSession);
            };
            let Some(turn) = event.turn_id.clone() else {
                return Err(EventSequenceError::TurnWithoutSession);
            };
            self.terminal_by_turn.insert((session, turn), true);
        }
        self.next_by_scope.insert(scope, expected + 1);
        self.events.push(event);
        Ok(())
    }

    /// Returns the immutable ordered event sequence.
    #[must_use]
    pub fn events(&self) -> &[HarnessEvent] {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(sequence: u64, turn: &str) -> HarnessEvent {
        HarnessEvent {
            schema_version: EventSchemaVersion::CURRENT,
            event_id: EventId::new(format!("event-{turn}-{sequence}")).unwrap(),
            correlation_id: None,
            session_id: Some(SessionId::new("session").unwrap()),
            turn_id: Some(TurnId::new(turn).unwrap()),
            sequence: EventSequence::new(sequence),
            payload: HarnessEventPayload::TurnCompleted {
                outcome: FinishOutcome::Stop,
                metadata: ExtensionMap::default(),
            },
        }
    }

    #[test]
    fn event_log_scopes_sequences_to_ownership() {
        let mut log = EventLog::default();
        log.push(event(0, "turn-a")).unwrap();
        log.push(event(0, "turn-b")).unwrap();
        assert!(matches!(
            log.push(event(2, "turn-c")),
            Err(EventSequenceError::NonMonotonic { .. })
        ));
    }

    #[test]
    fn terminal_events_are_unique_per_turn() {
        let mut log = EventLog::default();
        log.push(event(0, "turn")).unwrap();
        let mut duplicate = event(1, "turn");
        duplicate.event_id = EventId::new("other-event").unwrap();
        assert_eq!(
            log.push(duplicate),
            Err(EventSequenceError::DuplicateTurnTerminal)
        );
    }
}
