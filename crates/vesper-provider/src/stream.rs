use serde::{Deserialize, Serialize};
use thiserror::Error;

use vesper_domain::{
    BoundedString, ContentPart, ContentText, ExtensionMap, FinishOutcome, NormalizedUsage,
    ProviderResponseId, ProviderToolName, ReasoningKind, ReasoningRetention, SafeMessage, ToolCall,
    ToolCallId,
};

use crate::ProviderError;

/// Normalized quota/rate status without assuming one provider header schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimitUpdate {
    /// Optional remaining units.
    pub remaining: Option<u64>,
    /// Optional reset delay.
    pub reset_after_ms: Option<u64>,
    /// Namespaced provider metadata.
    #[serde(default)]
    pub metadata: ExtensionMap,
}

/// Normalized quota status distinct from short-window rate limiting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaUpdate {
    /// Optional remaining quota units.
    pub remaining: Option<u64>,
    /// Optional total quota units.
    pub limit: Option<u64>,
    /// Optional reset delay.
    pub reset_after_ms: Option<u64>,
    /// Namespaced provider metadata.
    #[serde(default)]
    pub metadata: ExtensionMap,
}

/// Ordered provider output before harness session/turn sequencing is applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "kebab-case")]
pub enum ProviderStreamEvent {
    /// Response accepted.
    ResponseStarted {
        /// Provider response identity where available.
        response_id: Option<ProviderResponseId>,
        /// Safe response/request metadata.
        metadata: ExtensionMap,
    },
    /// Exposed or opaque reasoning record.
    ReasoningDelta {
        /// Stream identifier.
        stream_id: BoundedString<128>,
        /// Text or opaque reference.
        text: ContentText,
        /// Reasoning kind.
        kind: ReasoningKind,
        /// Retention instruction.
        retention: ReasoningRetention,
    },
    /// User-visible content.
    ContentDelta {
        /// Stream identifier.
        stream_id: BoundedString<128>,
        /// Content part.
        part: ContentPart,
    },
    /// Tool call began.
    ToolCallStarted {
        /// Provider call index.
        index: u32,
        /// Optional call ID.
        call_id: Option<ToolCallId>,
        /// Optional name.
        name: Option<ProviderToolName>,
    },
    /// Incremental tool arguments.
    ToolCallDelta {
        /// Provider call index.
        index: u32,
        /// ID fragment.
        id_fragment: Option<BoundedString<256>>,
        /// Name fragment.
        name_fragment: Option<BoundedString<128>>,
        /// Argument fragment.
        arguments_fragment: ContentText,
    },
    /// Fully assembled call.
    ToolCallCompleted(ToolCall),
    /// Normalized usage.
    Usage(NormalizedUsage),
    /// Quota/rate status.
    RateLimit(RateLimitUpdate),
    /// Longer-window quota status.
    Quota(QuotaUpdate),
    /// Non-terminal provider warning.
    Warning {
        /// Safe warning text.
        message: SafeMessage,
        /// Safe provider metadata.
        metadata: ExtensionMap,
    },
    /// Exactly one normal terminal event.
    Completed {
        /// Finish classification.
        finish: FinishOutcome,
        /// Safe provider metadata.
        metadata: ExtensionMap,
    },
}

/// Stream invariant violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderStreamContractError {
    /// Event or error arrived after terminal state.
    #[error("provider stream emitted data after its terminal outcome")]
    OutputAfterTerminal,
    /// Stream ended without a terminal event/error.
    #[error("provider stream ended without a terminal outcome")]
    StreamEndedWithoutTerminal,
    /// A second terminal state was attempted.
    #[error("provider stream emitted more than one terminal outcome")]
    DuplicateTerminalOutcome,
}

/// Stateful validator for ordering-independent terminal/visibility invariants.
#[derive(Debug, Clone, Default)]
pub struct ProviderStreamContract {
    terminal: bool,
    visible_output: bool,
}

impl ProviderStreamContract {
    /// Accepts one ordered event.
    pub fn accept_event(
        &mut self,
        event: &ProviderStreamEvent,
    ) -> Result<(), ProviderStreamContractError> {
        if self.terminal {
            return Err(ProviderStreamContractError::OutputAfterTerminal);
        }
        match event {
            ProviderStreamEvent::ReasoningDelta {
                kind: ReasoningKind::ProviderVisible | ReasoningKind::Summary,
                ..
            }
            | ProviderStreamEvent::ContentDelta { .. }
            | ProviderStreamEvent::ToolCallStarted { .. }
            | ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallCompleted(_) => self.visible_output = true,
            ProviderStreamEvent::Completed { .. } => self.terminal = true,
            ProviderStreamEvent::ResponseStarted { .. }
            | ProviderStreamEvent::ReasoningDelta {
                kind: ReasoningKind::OpaqueContinuation,
                ..
            }
            | ProviderStreamEvent::Usage(_)
            | ProviderStreamEvent::RateLimit(_)
            | ProviderStreamEvent::Quota(_)
            | ProviderStreamEvent::Warning { .. } => {}
        }
        Ok(())
    }

    /// Accepts one classified terminal error.
    pub fn accept_error(
        &mut self,
        error: &ProviderError,
    ) -> Result<(), ProviderStreamContractError> {
        if self.terminal {
            return Err(ProviderStreamContractError::DuplicateTerminalOutcome);
        }
        self.visible_output |= error.info.visible_output_emitted;
        self.terminal = true;
        Ok(())
    }

    /// Validates end-of-stream.
    pub fn finish(self) -> Result<(), ProviderStreamContractError> {
        if self.terminal {
            Ok(())
        } else {
            Err(ProviderStreamContractError::StreamEndedWithoutTerminal)
        }
    }

    /// Whether a visible reasoning/content/tool event has escaped.
    #[must_use]
    pub const fn visible_output_emitted(&self) -> bool {
        self.visible_output
    }

    /// Whether terminal state has been reached.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        self.terminal
    }
}

#[cfg(test)]
mod tests {
    use vesper_domain::{FinishOutcome, SafeMessage};

    use super::*;

    #[test]
    fn exactly_one_terminal_outcome_is_required() {
        let mut contract = ProviderStreamContract::default();
        assert_eq!(
            contract.clone().finish(),
            Err(ProviderStreamContractError::StreamEndedWithoutTerminal)
        );
        contract
            .accept_event(&ProviderStreamEvent::Completed {
                finish: FinishOutcome::Stop,
                metadata: ExtensionMap::default(),
            })
            .unwrap();
        assert!(contract.clone().finish().is_ok());
        assert_eq!(
            contract.accept_event(&ProviderStreamEvent::Completed {
                finish: FinishOutcome::Stop,
                metadata: ExtensionMap::default(),
            }),
            Err(ProviderStreamContractError::OutputAfterTerminal)
        );
    }

    #[test]
    fn visible_partial_output_is_tracked_before_interruption() {
        let mut contract = ProviderStreamContract::default();
        contract
            .accept_event(&ProviderStreamEvent::ContentDelta {
                stream_id: BoundedString::new("content").unwrap(),
                part: ContentPart::Text(vesper_domain::ContentText::new("partial").unwrap()),
            })
            .unwrap();
        assert!(contract.visible_output_emitted());
        let _safe = SafeMessage::new("transport interrupted").unwrap();
    }
}
