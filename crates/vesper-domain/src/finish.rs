use serde::{Deserialize, Serialize};

/// Provider-neutral reason a response stream ended before its terminal event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamInterruptionCause {
    /// The bounded absolute generation deadline elapsed while data could still
    /// have been arriving.
    GenerationDeadline,
    /// No response bytes arrived within the configured inactivity window.
    ReadInactivity,
    /// The remote peer closed a successful response before its terminal event.
    RemoteEof,
    /// The HTTP transport failed after the response had started.
    #[default]
    Transport,
}

/// Terminal outcome for one provider/harness turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FinishOutcome {
    /// Normal completion.
    Stop,
    /// Provider requested tool execution.
    ToolCalls,
    /// Provider output limit reached.
    OutputLimit,
    /// Context capacity reached.
    ContextLimit,
    /// Safety policy stopped generation.
    Safety,
    /// Explicit cancellation.
    Cancelled,
    /// Transport ended after visible output. `tool_call_started` makes the
    /// no-replay boundary explicit: a partial tool call may have side effects
    /// and cannot be continued automatically.
    #[serde(alias = "network-interruption-after-visible-output")]
    StreamInterrupted {
        /// Classified interruption source.
        #[serde(default)]
        cause: StreamInterruptionCause,
        /// Whether any provider tool-call fragment was observed.
        #[serde(default)]
        tool_call_started: bool,
    },
    /// Provider returned a classified error.
    ProviderError,
    /// Provider stream or adapter protocol was malformed.
    ProtocolError,
    /// Provider value not understood by the adapter.
    UnknownProviderValue {
        /// Exact opaque provider value.
        raw: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_visible_network_interruption_deserializes_safely() {
        let outcome: FinishOutcome =
            serde_json::from_str(r#"{"kind":"network-interruption-after-visible-output"}"#)
                .unwrap();
        assert_eq!(
            outcome,
            FinishOutcome::StreamInterrupted {
                cause: StreamInterruptionCause::Transport,
                tool_call_started: false,
            }
        );
    }
}
