use serde::{Deserialize, Serialize};

use vesper_domain::{FeatureRequirement, SafeMessage};

/// Typed support level with optional native/emulated detail.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "support", rename_all = "kebab-case")]
pub enum SupportLevel<T> {
    /// Provider/runtime implements the feature directly.
    Native {
        /// Capability-specific details.
        details: T,
    },
    /// Adapter can emulate with explicit caveats.
    Emulated {
        /// Capability-specific details.
        details: T,
        /// User-visible caveat.
        caveat: SafeMessage,
    },
    /// Capability is known to be unavailable.
    Unsupported {
        /// Safe explanation.
        reason: SafeMessage,
    },
    /// Adapter cannot establish support.
    #[default]
    Unknown,
}

impl<T> SupportLevel<T> {
    /// Resolves a feature request without dispatching provider work.
    #[must_use]
    pub const fn resolve(
        &self,
        requirement: FeatureRequirement,
        fallback_available: bool,
    ) -> CapabilityResolution {
        resolve_support(self, requirement, fallback_available)
    }
}

/// Result of applying Require/Prefer/AllowFallback to support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityResolution {
    /// Use provider-native support.
    Native,
    /// Use adapter emulation.
    Emulated,
    /// Use a declared fallback and emit an observable fallback event.
    Fallback,
    /// Omit an unavailable preferred feature and report the omission.
    OmitPreferred,
    /// Reject before dispatch.
    Reject,
}

/// Resolves caller intent without dispatching a provider request.
#[must_use]
pub const fn resolve_support<T>(
    support: &SupportLevel<T>,
    requirement: FeatureRequirement,
    fallback_available: bool,
) -> CapabilityResolution {
    match support {
        SupportLevel::Native { .. } => CapabilityResolution::Native,
        SupportLevel::Emulated { .. } => CapabilityResolution::Emulated,
        SupportLevel::Unsupported { .. } | SupportLevel::Unknown => match requirement {
            FeatureRequirement::Require => CapabilityResolution::Reject,
            FeatureRequirement::Prefer => CapabilityResolution::OmitPreferred,
            FeatureRequirement::AllowFallback if fallback_available => {
                CapabilityResolution::Fallback
            }
            FeatureRequirement::AllowFallback => CapabilityResolution::Reject,
        },
    }
}

/// Context/output model limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimits {
    /// Maximum context tokens.
    pub context_tokens: Option<u64>,
    /// Maximum output tokens.
    pub output_tokens: Option<u64>,
    /// Whether values are provider-exact.
    pub exact: bool,
}

/// Reasoning support without treating hidden chain-of-thought as portable content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningCapability {
    /// Provider-supported effort labels.
    pub effort_levels: Vec<String>,
    /// Provider-supported visible reasoning modes.
    pub visible_modes: Vec<String>,
}

/// Streamed reasoning forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamedReasoningCapability {
    /// Provider-visible text deltas.
    pub visible_text: bool,
    /// Displayable summary deltas.
    pub summaries: bool,
}

/// Provider reasoning persistence/continuation forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreservedReasoningCapability {
    /// Provider-visible blocks can be round-tripped.
    pub visible_blocks: bool,
    /// Opaque non-display continuation records can be round-tripped.
    pub opaque_records: bool,
}

/// Image/audio limits and accepted forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaCapability {
    /// Accepted media types.
    pub media_types: Vec<String>,
    /// Maximum items per request.
    pub maximum_items: Option<u32>,
    /// External references accepted.
    pub references: bool,
    /// Inline descriptors accepted.
    pub inline_data: bool,
}

/// Tool schema/choice/stream behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCapability {
    /// Adapter-owned schema dialect ID.
    pub schema_dialect: String,
    /// Supported tool-choice modes.
    pub choice_modes: Vec<String>,
    /// Parallel calls.
    pub parallel: bool,
    /// Incremental arguments.
    pub streamed_arguments: bool,
}

/// Tool-choice modes supported by one adapter/model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolChoiceCapability {
    /// Auto-selection.
    pub automatic: bool,
    /// Explicit no-tools mode.
    pub none: bool,
    /// At-least-one mode.
    pub required: bool,
    /// Named-tool selection.
    pub named: bool,
}

/// Prompt-cache behavior and metric support.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCacheCapability {
    /// Supported cache controls.
    pub controls: Vec<String>,
    /// Provider reports cache-read usage.
    pub reports_reads: bool,
    /// Provider reports cache-write usage.
    pub reports_writes: bool,
}

/// Continuation modes exposed by the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationCapability {
    /// Stable strategy IDs understood by the adapter.
    pub strategies: Vec<String>,
    /// Provider-defined maximum where known.
    pub provider_maximum: Option<u32>,
}

/// External runtime ownership (for example a process-backed CLI provider).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalRuntimeCapability {
    /// Runtime owns tool execution.
    pub owns_tools: bool,
    /// Runtime owns permission prompts.
    pub owns_permissions: bool,
    /// Runtime can be cancelled by the harness.
    pub cancellable: bool,
}

/// Structured-output modes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredOutputCapability {
    /// JSON-object mode.
    pub json_mode: bool,
    /// JSON Schema constrained output.
    pub json_schema: bool,
    /// Provider grammar identifiers.
    pub grammars: Vec<String>,
}

/// Authentication styles an adapter may contribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationCapability {
    /// Stable auth method IDs.
    pub methods: Vec<String>,
    /// Whether no authentication is a valid configured mode.
    pub optional: bool,
}

/// Typed provider capability snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Context/output limits.
    pub limits: SupportLevel<ModelLimits>,
    /// Visible/opaque reasoning support.
    pub reasoning: SupportLevel<ReasoningCapability>,
    /// Streamed visible reasoning support.
    pub streamed_reasoning: SupportLevel<StreamedReasoningCapability>,
    /// Preserved/opaque reasoning support.
    pub preserved_reasoning: SupportLevel<PreservedReasoningCapability>,
    /// Vision.
    pub vision: SupportLevel<MediaCapability>,
    /// Audio.
    pub audio: SupportLevel<MediaCapability>,
    /// Tools.
    pub tools: SupportLevel<ToolCapability>,
    /// Tool-choice controls.
    pub tool_choice: SupportLevel<ToolChoiceCapability>,
    /// Parallel tool calls.
    pub parallel_tool_calls: SupportLevel<()>,
    /// Streamed argument fragments.
    pub streamed_tool_arguments: SupportLevel<()>,
    /// Prompt caching.
    pub prompt_caching: SupportLevel<PromptCacheCapability>,
    /// Structured output.
    pub structured_output: SupportLevel<StructuredOutputCapability>,
    /// Explicit sampling controls.
    pub sampling: SupportLevel<Vec<String>>,
    /// Dynamic model discovery.
    pub model_discovery: SupportLevel<Vec<String>>,
    /// Authentication.
    pub authentication: SupportLevel<AuthenticationCapability>,
    /// Quota/rate reporting.
    pub quota_reporting: SupportLevel<Vec<String>>,
    /// Continuation modes.
    pub continuation: SupportLevel<ContinuationCapability>,
    /// Process-backed transport.
    pub process_backed: SupportLevel<Vec<String>>,
    /// Provider/external runtime ownership of tools or permissions.
    pub external_runtime: SupportLevel<ExternalRuntimeCapability>,
}

impl ProviderCapabilities {
    /// Resolves a stable shared capability ID.
    ///
    /// Unknown names remain `Unknown` and therefore cannot satisfy `Require`.
    #[must_use]
    pub fn resolve(
        &self,
        capability: &str,
        requirement: FeatureRequirement,
        fallback_available: bool,
    ) -> CapabilityResolution {
        match capability {
            "provider:limits" => self.limits.resolve(requirement, fallback_available),
            "provider:reasoning" => self.reasoning.resolve(requirement, fallback_available),
            "provider:streamed-reasoning" => self
                .streamed_reasoning
                .resolve(requirement, fallback_available),
            "provider:preserved-reasoning" => self
                .preserved_reasoning
                .resolve(requirement, fallback_available),
            "provider:vision" => self.vision.resolve(requirement, fallback_available),
            "provider:audio" => self.audio.resolve(requirement, fallback_available),
            "provider:tools" => self.tools.resolve(requirement, fallback_available),
            "provider:tool-choice" => self.tool_choice.resolve(requirement, fallback_available),
            "provider:parallel-tools" => self
                .parallel_tool_calls
                .resolve(requirement, fallback_available),
            "provider:streamed-tool-arguments" => self
                .streamed_tool_arguments
                .resolve(requirement, fallback_available),
            "provider:prompt-caching" => {
                self.prompt_caching.resolve(requirement, fallback_available)
            }
            "provider:structured-output" => self
                .structured_output
                .resolve(requirement, fallback_available),
            "provider:sampling" => self.sampling.resolve(requirement, fallback_available),
            "provider:model-discovery" => self
                .model_discovery
                .resolve(requirement, fallback_available),
            "provider:authentication" => {
                self.authentication.resolve(requirement, fallback_available)
            }
            "provider:quota-reporting" => self
                .quota_reporting
                .resolve(requirement, fallback_available),
            "provider:continuation" => self.continuation.resolve(requirement, fallback_available),
            "provider:process-backed" => {
                self.process_backed.resolve(requirement, fallback_available)
            }
            "provider:external-runtime" => self
                .external_runtime
                .resolve(requirement, fallback_available),
            _ => resolve_support(
                &SupportLevel::<()>::Unknown,
                requirement,
                fallback_available,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use vesper_domain::FeatureRequirement;

    use super::*;

    #[test]
    fn requirement_matrix_is_explicit() {
        let unsupported: SupportLevel<()> = SupportLevel::Unsupported {
            reason: SafeMessage::new("not available").unwrap(),
        };
        assert_eq!(
            resolve_support(&unsupported, FeatureRequirement::Require, true),
            CapabilityResolution::Reject
        );
        assert_eq!(
            resolve_support(&unsupported, FeatureRequirement::Prefer, false),
            CapabilityResolution::OmitPreferred
        );
        assert_eq!(
            resolve_support(&unsupported, FeatureRequirement::AllowFallback, true),
            CapabilityResolution::Fallback
        );
        assert_eq!(
            resolve_support(&unsupported, FeatureRequirement::AllowFallback, false),
            CapabilityResolution::Reject
        );
    }
}
