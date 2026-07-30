use std::collections::BTreeSet;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use vesper_domain::{
    ErrorCategory, EventLog, HarnessEvent, HarnessEventPayload, ToolCallId, ToolResult,
};
use vesper_provider::{ProviderError, ProviderStreamContract, ProviderStreamEvent};

/// Contract assertion failure for reusable runtime-stage tests.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConformanceError {
    /// Event sequencing/ownership failed.
    #[error("harness event sequence contract failed: {0}")]
    EventSequence(String),
    /// Stream terminal-state invariant failed.
    #[error("provider terminal contract failed: {0}")]
    ProviderTerminal(String),
    /// Tool result has no preceding completed call.
    #[error("tool result references unknown call {0}")]
    BrokenToolLinkage(String),
    /// A completed call has no result where pairing is required.
    #[error("tool call has no linked result {0}")]
    MissingToolResult(String),
    /// Expected cancellation was classified as another failure.
    #[error("terminal error is not cancellation")]
    CancellationMisclassified,
    /// Raw canary escaped.
    #[error("secret canary leaked into canonical output")]
    CanaryLeak,
    /// Decode/encode changed a value that must be preserved.
    #[error("compatibility round trip changed canonical JSON")]
    RoundTripLoss,
    /// Bounded event sink rejected additional output.
    #[error("bounded event sink capacity exceeded")]
    SinkCapacityExceeded,
}

/// Applies the shared event log validator to an ordered event slice.
pub fn assert_harness_event_order(events: &[HarnessEvent]) -> Result<(), ConformanceError> {
    let mut log = EventLog::default();
    for event in events {
        log.push(event.clone())
            .map_err(|error| ConformanceError::EventSequence(error.to_string()))?;
    }
    Ok(())
}

/// Validates exactly one provider terminal event/error and visible-output tracking.
pub fn assert_provider_stream_contract(
    items: &[Result<ProviderStreamEvent, ProviderError>],
) -> Result<bool, ConformanceError> {
    let mut contract = ProviderStreamContract::default();
    for item in items {
        match item {
            Ok(event) => contract
                .accept_event(event)
                .map_err(|error| ConformanceError::ProviderTerminal(error.to_string()))?,
            Err(error) => contract
                .accept_error(error)
                .map_err(|error| ConformanceError::ProviderTerminal(error.to_string()))?,
        }
    }
    let visible = contract.visible_output_emitted();
    contract
        .finish()
        .map_err(|error| ConformanceError::ProviderTerminal(error.to_string()))?;
    Ok(visible)
}

/// Requires all completed tool calls in the event slice to have exactly one linked result.
pub fn assert_tool_call_result_linkage(events: &[HarnessEvent]) -> Result<(), ConformanceError> {
    let mut calls = BTreeSet::<ToolCallId>::new();
    let mut results = BTreeSet::<ToolCallId>::new();
    for event in events {
        match &event.payload {
            HarnessEventPayload::ToolCallCompleted(call) => {
                calls.insert(call.id.clone());
            }
            HarnessEventPayload::ToolResultCompleted(ToolResult { call_id, .. })
                if !calls.contains(call_id) || !results.insert(call_id.clone()) =>
            {
                return Err(ConformanceError::BrokenToolLinkage(call_id.to_string()));
            }
            _ => {}
        }
    }
    if let Some(missing) = calls.difference(&results).next() {
        return Err(ConformanceError::MissingToolResult(missing.to_string()));
    }
    Ok(())
}

/// Asserts a provider terminal error retains cancellation classification.
pub fn assert_cancellation(error: &ProviderError) -> Result<(), ConformanceError> {
    if error.info.category == ErrorCategory::Cancellation {
        Ok(())
    } else {
        Err(ConformanceError::CancellationMisclassified)
    }
}

/// Rejects a raw canary anywhere in serializable output.
pub fn assert_secret_canary_absent<T: Serialize>(
    value: &T,
    canary: &str,
) -> Result<(), ConformanceError> {
    let encoded = serde_json::to_string(value).map_err(|_| ConformanceError::CanaryLeak)?;
    if encoded.contains(canary) {
        Err(ConformanceError::CanaryLeak)
    } else {
        Ok(())
    }
}

/// Proves a serialization contract preserves unknown fields and ordering.
pub fn assert_json_round_trip<T>(value: &Value) -> Result<(), ConformanceError>
where
    T: Serialize + DeserializeOwned,
{
    let decoded: T =
        serde_json::from_value(value.clone()).map_err(|_| ConformanceError::RoundTripLoss)?;
    let encoded = serde_json::to_value(decoded).map_err(|_| ConformanceError::RoundTripLoss)?;
    if encoded == *value {
        Ok(())
    } else {
        Err(ConformanceError::RoundTripLoss)
    }
}

/// Deterministic bounded in-memory event sink.
#[derive(Debug, Clone)]
pub struct BoundedEventSink {
    maximum: usize,
    events: Vec<HarnessEvent>,
}

impl BoundedEventSink {
    /// Creates a nonzero-capacity sink.
    #[must_use]
    pub fn new(maximum: usize) -> Self {
        Self {
            maximum,
            events: Vec::new(),
        }
    }

    /// Adds one event or fails explicitly when capacity is exhausted.
    pub fn push(&mut self, event: HarnessEvent) -> Result<(), ConformanceError> {
        if self.events.len() >= self.maximum {
            return Err(ConformanceError::SinkCapacityExceeded);
        }
        self.events.push(event);
        Ok(())
    }

    /// Returns captured events in insertion order.
    #[must_use]
    pub fn events(&self) -> &[HarnessEvent] {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vesper_domain::{
        CommandId, CommandInitiator, CommandSchemaVersion, CorrelationId, EventId,
        EventSchemaVersion, EventSequence, ExtensionMap, FinishOutcome, FragmentedToolCallIdentity,
        HarnessCommand, HarnessCommandPayload, HarnessEvent, HarnessEventPayload,
        LegacySessionError, LegacySessionV1, PermissionRequestId, ProviderToolName, Revision,
        SessionId, ToolCall, ToolCallId, ToolId, ToolResult, ToolResultId, ToolResultStatus,
        TurnId, UsageMeasurement, UsageMode, UsageProvenance,
    };
    use vesper_provider::{CapabilityResolution, SupportLevel, resolve_support};

    use crate::{FixtureCorpus, fixture_root};

    use super::*;

    #[test]
    fn raw_canary_is_rejected() {
        assert_eq!(
            assert_secret_canary_absent(
                &"VESPER_SECRET_CANARY_7xQ9m2Kp",
                "VESPER_SECRET_CANARY_7xQ9m2Kp"
            ),
            Err(ConformanceError::CanaryLeak)
        );
    }

    #[test]
    fn bounded_sink_never_discards_silently() {
        let sink = BoundedEventSink::new(0);
        assert!(sink.events().is_empty());
    }

    #[test]
    fn synthetic_contract_vectors_are_all_present_and_exact() {
        let corpus = FixtureCorpus::load(fixture_root()).unwrap();
        let contracts = corpus
            .scenarios
            .iter()
            .filter(|scenario| scenario.manifest.category == "contracts")
            .collect::<Vec<_>>();
        assert_eq!(contracts.len(), 11);
        for fixture in contracts {
            assert_eq!(
                fixture.manifest.expected_state, fixture.result.final_state,
                "{}",
                fixture.manifest.scenario_id
            );
            assert_eq!(
                fixture.manifest.input["synthetic_future_contract"],
                Value::Bool(true)
            );
        }
    }

    #[test]
    fn command_event_correlation_and_message_identity_round_trip() {
        let command = HarnessCommand {
            schema_version: CommandSchemaVersion::CURRENT,
            command_id: CommandId::new("command-1").unwrap(),
            correlation_id: CorrelationId::new("correlation-1").unwrap(),
            initiator: CommandInitiator::Acp,
            expected_revision: Some(Revision::new(3)),
            payload: HarnessCommandPayload::ProvidePermissionDecision {
                session_id: SessionId::new("session-1").unwrap(),
                request_id: PermissionRequestId::new("permission-1").unwrap(),
                outcome: vesper_domain::PermissionOutcome::RejectOnce,
            },
        };
        let decoded: HarnessCommand =
            serde_json::from_value(serde_json::to_value(&command).unwrap()).unwrap();
        assert_eq!(decoded, command);

        let event = HarnessEvent {
            schema_version: EventSchemaVersion::CURRENT,
            event_id: EventId::new("event-1").unwrap(),
            correlation_id: Some(command.correlation_id),
            session_id: Some(SessionId::new("session-1").unwrap()),
            turn_id: Some(TurnId::new("turn-1").unwrap()),
            sequence: EventSequence::new(0),
            payload: HarnessEventPayload::UserMessageAccepted {
                message_id: vesper_domain::MessageId::new("message-1").unwrap(),
            },
        };
        assert_harness_event_order(&[event]).unwrap();
    }

    #[test]
    fn unknown_extensions_finish_reasons_and_tool_fragment_identity_round_trip() {
        let mut extensions = ExtensionMap::default();
        extensions
            .insert("future.example:field", json!({"preserve": [1, 2, 3]}))
            .unwrap();
        let encoded = serde_json::to_value(&extensions).unwrap();
        let decoded: ExtensionMap = serde_json::from_value(encoded).unwrap();
        assert_eq!(
            decoded.get("future.example:field"),
            Some(&json!({"preserve": [1, 2, 3]}))
        );

        let finish = FinishOutcome::UnknownProviderValue {
            raw: "future_finish_reason".into(),
        };
        assert_eq!(
            serde_json::from_value::<FinishOutcome>(serde_json::to_value(&finish).unwrap())
                .unwrap(),
            finish
        );

        let identity = FragmentedToolCallIdentity {
            stream_index: 1,
            call_id: Some(ToolCallId::new("call-b").unwrap()),
            provider_name: Some(ProviderToolName::new("future_tool").unwrap()),
        };
        assert_eq!(
            serde_json::from_value::<FragmentedToolCallIdentity>(
                serde_json::to_value(&identity).unwrap()
            )
            .unwrap(),
            identity
        );
    }

    #[test]
    fn usage_modes_and_provenance_never_collapse() {
        let exact = UsageMeasurement::exact(3);
        let estimated = UsageMeasurement {
            value: Some(7),
            provenance: UsageProvenance::Estimated,
        };
        let unavailable = UsageMeasurement::unavailable();
        assert_ne!(exact, estimated);
        assert_ne!(estimated, unavailable);
        assert_ne!(UsageMode::Delta, UsageMode::Cumulative);
    }

    #[test]
    fn fallback_and_terminal_algebras_match_contract_vectors() {
        let unsupported: SupportLevel<()> = SupportLevel::Unsupported {
            reason: vesper_domain::SafeMessage::new("unsupported").unwrap(),
        };
        assert_eq!(
            resolve_support(
                &unsupported,
                vesper_domain::FeatureRequirement::AllowFallback,
                true
            ),
            CapabilityResolution::Fallback
        );

        let events = [Ok(ProviderStreamEvent::Completed {
            finish: FinishOutcome::Stop,
            metadata: ExtensionMap::default(),
        })];
        assert!(!assert_provider_stream_contract(&events).unwrap());
        let duplicate = [
            events[0].clone(),
            Ok(ProviderStreamEvent::Completed {
                finish: FinishOutcome::Stop,
                metadata: ExtensionMap::default(),
            }),
        ];
        assert!(assert_provider_stream_contract(&duplicate).is_err());
    }

    #[test]
    fn invalid_legacy_bound_is_an_explicit_compatibility_error() {
        let oversized = format!(r#"{{"task_context":"{}"}}"#, "x".repeat(2_001));
        assert!(matches!(
            LegacySessionV1::decode_json(oversized.as_bytes()),
            Err(LegacySessionError::BoundedValue {
                field: "task_context",
                maximum: 2_000
            })
        ));
    }

    #[test]
    fn tool_linkage_accepts_one_pair_and_rejects_an_orphan() {
        let call_id = ToolCallId::new("call-1").unwrap();
        let call = ToolCall {
            id: call_id.clone(),
            tool_id: ToolId::new("tool-1").unwrap(),
            arguments: json!({"path": "README.md"}),
            extensions: ExtensionMap::default(),
        };
        let result = ToolResult {
            id: ToolResultId::new("result-1").unwrap(),
            call_id: call_id.clone(),
            output: json!({"content": "ok"}),
            status: ToolResultStatus::Succeeded,
            locations: Vec::new(),
            diff_summary: None,
            extensions: ExtensionMap::default(),
        };
        let envelope = |sequence, payload| HarnessEvent {
            schema_version: EventSchemaVersion::CURRENT,
            event_id: EventId::new(format!("event-{sequence}")).unwrap(),
            correlation_id: None,
            session_id: Some(SessionId::new("session-1").unwrap()),
            turn_id: Some(TurnId::new("turn-1").unwrap()),
            sequence: EventSequence::new(sequence),
            payload,
        };
        let linked = [
            envelope(0, HarnessEventPayload::ToolCallCompleted(call)),
            envelope(1, HarnessEventPayload::ToolResultCompleted(result.clone())),
        ];
        assert_tool_call_result_linkage(&linked).unwrap();
        assert_eq!(
            assert_tool_call_result_linkage(&[envelope(
                0,
                HarnessEventPayload::ToolResultCompleted(result)
            )]),
            Err(ConformanceError::BrokenToolLinkage(call_id.to_string()))
        );
    }
}
