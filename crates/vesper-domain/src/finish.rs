use serde::{Deserialize, Serialize};

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
    /// Transport ended after user-visible output.
    NetworkInterruptionAfterVisibleOutput,
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
