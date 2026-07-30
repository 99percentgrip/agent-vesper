use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use vesper_domain::{
    BoundedString, CapabilityId, CapabilityRequest, ContentPart, ConversationMessage, EndpointId,
    ExtensionMap, ProviderId, ProviderRequestId, QualifiedModelId, ReasoningRetention, SafeMessage,
    SystemInstruction, ToolChoiceIntent, ToolDefinition, VersionedExtensionEnvelope,
};

use crate::{CapabilityResolution, ProviderCapabilities};

/// Tool selection intent. Adapters map this to their dialect.
pub type ToolChoice = ToolChoiceIntent;

/// Purpose of a bounded auxiliary request through the same provider abstraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuxiliaryRequestIntent {
    /// Context compaction summary.
    Compaction,
    /// Advisory permission review.
    PermissionReview,
    /// Verification critic.
    Verification,
    /// Explicit media fallback.
    MediaAnalysis,
}

/// Reason continuation is requested or denied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContinuationReason {
    /// Output limit with continuation permitted.
    OutputLimit,
    /// Provider cursor offered a resumable response.
    ProviderCursor,
    /// Harness safety maximum reached.
    HarnessMaximumReached,
    /// Provider maximum reached.
    ProviderMaximumReached,
    /// Visible output makes replay unsafe.
    ReplayUnsafeAfterVisibleOutput,
    /// Adapter-specific safe reason.
    ProviderDefined(SafeMessage),
}

/// Provider continuation behavior.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ContinuationStrategy {
    /// No continuation.
    Unsupported,
    /// Adapter adds a provider-specific continuation message.
    ReplayWithProviderMessage {
        /// Provider-owned continuation content.
        message: VersionedExtensionEnvelope,
    },
    /// Provider cursor/token preserved as opaque metadata.
    ProviderCursor {
        /// Cursor data.
        cursor: VersionedExtensionEnvelope,
    },
    /// Provider-native continuation.
    NativeContinuation {
        /// Provider continuation data.
        state: VersionedExtensionEnvelope,
    },
}

/// Bounded continuation state carried between requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContinuationContext {
    /// Selected strategy.
    pub strategy: ContinuationStrategy,
    /// Provider-defined maximum.
    pub provider_maximum: Option<u32>,
    /// Harness safety maximum.
    pub harness_maximum: u32,
    /// Continuations already visible to the user/runtime.
    pub visible_count: u32,
    /// Why continuation is currently permitted or denied.
    pub reason: ContinuationReason,
    /// Safe adapter metadata.
    #[serde(default)]
    pub metadata: ExtensionMap,
}

impl ContinuationContext {
    /// Returns whether both provider and harness bounds allow another continuation.
    #[must_use]
    pub fn may_continue(&self) -> bool {
        self.visible_count < self.harness_maximum
            && self
                .provider_maximum
                .is_none_or(|maximum| self.visible_count < maximum)
            && !matches!(
                self.reason,
                ContinuationReason::HarnessMaximumReached
                    | ContinuationReason::ProviderMaximumReached
                    | ContinuationReason::ReplayUnsafeAfterVisibleOutput
            )
            && !matches!(self.strategy, ContinuationStrategy::Unsupported)
    }
}

/// Global request fallback policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FallbackPolicy {
    /// Reject unsupported requested behavior.
    Strict,
    /// Apply only explicitly declared fallbacks.
    DeclaredOnly,
}

/// Desired provider-visible reasoning behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningIntent {
    /// Adapter-defined effort/mode label.
    pub mode: Option<BoundedString<128>>,
    /// Whether displayable reasoning should stream when supported.
    pub stream_visible: bool,
    /// Retention policy for provider-visible or opaque records.
    pub retention: ReasoningRetention,
}

/// Structured-output request independent of provider wire syntax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "schema", rename_all = "kebab-case")]
pub enum StructuredOutputIntent {
    /// No structured output requirement.
    None,
    /// Parseable JSON object.
    JsonObject,
    /// JSON Schema constrained result.
    JsonSchema(Value),
    /// Provider-owned structured format.
    ProviderExtension(VersionedExtensionEnvelope),
}

/// Optional sampling controls; adapters reject unsupported explicit fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SamplingIntent {
    /// Temperature.
    pub temperature: Option<f64>,
    /// Nucleus sampling.
    pub top_p: Option<f64>,
    /// Stable seed where supported.
    pub seed: Option<u64>,
    /// Provider-owned controls.
    #[serde(default)]
    pub extensions: ExtensionMap,
}

/// Provider-neutral request. No provider SDK or transport structures are permitted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderRequest {
    /// Request correlation identity.
    pub request_id: ProviderRequestId,
    /// Provider identity.
    pub provider_id: ProviderId,
    /// Provider-qualified opaque model.
    pub model: QualifiedModelId,
    /// Selected endpoint/profile.
    pub endpoint_id: Option<EndpointId>,
    /// Ordered system instructions.
    pub system_instructions: Vec<SystemInstruction>,
    /// Ordered conversation and multimodal parts.
    pub messages: Vec<ConversationMessage>,
    /// Normalized tools.
    pub tools: Vec<ToolDefinition>,
    /// Tool choice.
    pub tool_choice: ToolChoice,
    /// Capability intents validated before dispatch.
    pub capabilities: Vec<CapabilityRequest>,
    /// Reasoning intent.
    pub reasoning: Option<ReasoningIntent>,
    /// Structured output intent.
    pub structured_output: StructuredOutputIntent,
    /// Optional sampling controls.
    pub sampling: Option<SamplingIntent>,
    /// Optional output bound.
    pub maximum_output_tokens: Option<u64>,
    /// Optional continuation state.
    pub continuation: Option<ContinuationContext>,
    /// Global fallback policy.
    pub fallback_policy: FallbackPolicy,
    /// Provider-specific versioned request values.
    pub provider_extensions: Option<VersionedExtensionEnvelope>,
}

/// Provider-owned configuration contribution rendered by future frontends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfigContribution {
    /// Provider identity.
    pub provider_id: ProviderId,
    /// Contribution schema version.
    pub schema_version: u32,
    /// Non-secret JSON Schema.
    pub config_schema: Value,
    /// Stable secret-reference field IDs; never values.
    pub secret_reference_fields: Vec<BoundedString<128>>,
}

/// Observable consequence of resolving one requested feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackDecision {
    /// Stable capability identity.
    pub capability: CapabilityId,
    /// Resolution selected before dispatch.
    pub resolution: crate::CapabilityResolution,
    /// Safe description for event emission.
    pub explanation: SafeMessage,
}

/// Request rejected before provider dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RequestValidationError {
    /// Qualified model belongs to a different provider.
    #[error("request provider does not own the qualified model")]
    ModelProviderMismatch,
    /// Explicit control lacks a declared capability requirement.
    #[error("request control requires a capability intent for {0}")]
    MissingCapabilityIntent(String),
    /// Required provider support is unavailable.
    #[error("provider cannot satisfy required capability {0}")]
    UnsupportedRequiredCapability(String),
    /// Explicit numeric or continuation control is invalid.
    #[error("provider request control is invalid: {0}")]
    InvalidControl(&'static str),
}

impl ProviderRequest {
    /// Resolves every capability and explicit control before transport dispatch.
    pub fn validate_capabilities(
        &self,
        supported: &ProviderCapabilities,
    ) -> Result<Vec<FallbackDecision>, RequestValidationError> {
        if self.provider_id != self.model.provider_id {
            return Err(RequestValidationError::ModelProviderMismatch);
        }
        if self.maximum_output_tokens == Some(0) {
            return Err(RequestValidationError::InvalidControl(
                "maximum output must be nonzero",
            ));
        }
        if let Some(sampling) = &self.sampling {
            if sampling.temperature.is_some_and(|value| !value.is_finite()) {
                return Err(RequestValidationError::InvalidControl(
                    "temperature must be finite",
                ));
            }
            if sampling
                .top_p
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            {
                return Err(RequestValidationError::InvalidControl(
                    "top_p must be finite and between zero and one",
                ));
            }
        }
        if self
            .continuation
            .as_ref()
            .is_some_and(|continuation| continuation.harness_maximum == 0)
        {
            return Err(RequestValidationError::InvalidControl(
                "continuation maximum must be nonzero",
            ));
        }
        for capability in self.required_control_capabilities() {
            if !self
                .capabilities
                .iter()
                .any(|request| request.capability.as_str() == capability)
            {
                return Err(RequestValidationError::MissingCapabilityIntent(
                    capability.into(),
                ));
            }
        }

        let mut decisions = Vec::with_capacity(self.capabilities.len());
        for request in &self.capabilities {
            let capability = request.capability.as_str();
            let fallback_available =
                self.fallback_policy == FallbackPolicy::DeclaredOnly && request.fallback.is_some();
            let resolution = supported.resolve(capability, request.requirement, fallback_available);
            if resolution == CapabilityResolution::Reject {
                return Err(RequestValidationError::UnsupportedRequiredCapability(
                    capability.into(),
                ));
            }
            decisions.push(FallbackDecision {
                capability: request.capability.clone(),
                resolution,
                explanation: request
                    .fallback
                    .as_ref()
                    .filter(|_| resolution == CapabilityResolution::Fallback)
                    .map_or_else(
                        || {
                            SafeMessage::new(format!(
                                "capability {capability} resolved as {resolution:?}"
                            ))
                            .expect("bounded capability resolution")
                        },
                        |fallback| fallback.description.clone(),
                    ),
            });
        }
        Ok(decisions)
    }

    fn required_control_capabilities(&self) -> Vec<&'static str> {
        let mut capabilities = Vec::new();
        if !self.tools.is_empty() || !matches!(self.tool_choice, ToolChoice::None) {
            capabilities.push("provider:tools");
            capabilities.push("provider:tool-choice");
        }
        if self.reasoning.is_some() {
            capabilities.push("provider:reasoning");
            if self
                .reasoning
                .as_ref()
                .is_some_and(|intent| intent.stream_visible)
            {
                capabilities.push("provider:streamed-reasoning");
            }
        }
        if !matches!(self.structured_output, StructuredOutputIntent::None) {
            capabilities.push("provider:structured-output");
        }
        if self.sampling.is_some() {
            capabilities.push("provider:sampling");
        }
        if self.maximum_output_tokens.is_some() {
            capabilities.push("provider:limits");
        }
        if self
            .system_instructions
            .iter()
            .any(|instruction| instruction.cache_stable)
        {
            capabilities.push("provider:prompt-caching");
        }
        if self.continuation.is_some() {
            capabilities.push("provider:continuation");
        }
        if self
            .messages
            .iter()
            .flat_map(|message| &message.content)
            .chain(
                self.system_instructions
                    .iter()
                    .flat_map(|instruction| &instruction.content),
            )
            .any(|part| matches!(part, ContentPart::Image(_)))
        {
            capabilities.push("provider:vision");
        }
        if self
            .messages
            .iter()
            .flat_map(|message| &message.content)
            .chain(
                self.system_instructions
                    .iter()
                    .flat_map(|instruction| &instruction.content),
            )
            .any(|part| matches!(part, ContentPart::Audio(_)))
        {
            capabilities.push("provider:audio");
        }
        capabilities
    }
}

#[cfg(test)]
mod tests {
    use vesper_domain::{
        CapabilityFallback, CapabilityId, FeatureRequirement, ModelId, SafeMessage,
    };

    use super::*;

    fn request(
        requirement: FeatureRequirement,
        fallback_policy: FallbackPolicy,
    ) -> ProviderRequest {
        ProviderRequest {
            request_id: ProviderRequestId::new("request-1").unwrap(),
            provider_id: ProviderId::new("test.provider").unwrap(),
            model: QualifiedModelId {
                provider_id: ProviderId::new("test.provider").unwrap(),
                model_id: ModelId::new("test-model").unwrap(),
            },
            endpoint_id: None,
            system_instructions: Vec::new(),
            messages: Vec::new(),
            tools: Vec::new(),
            tool_choice: ToolChoice::None,
            capabilities: vec![CapabilityRequest {
                capability: CapabilityId::new("provider:structured-output").unwrap(),
                requirement,
                fallback: (requirement == FeatureRequirement::AllowFallback).then(|| {
                    CapabilityFallback {
                        id: CapabilityId::new("fallback:plain-json").unwrap(),
                        description: SafeMessage::new("use plain JSON parsing").unwrap(),
                    }
                }),
            }],
            reasoning: None,
            structured_output: StructuredOutputIntent::JsonObject,
            sampling: None,
            maximum_output_tokens: None,
            continuation: None,
            fallback_policy,
            provider_extensions: None,
        }
    }

    #[test]
    fn unsupported_explicit_control_is_rejected_before_dispatch() {
        let supported = ProviderCapabilities::default();
        assert_eq!(
            request(FeatureRequirement::Require, FallbackPolicy::DeclaredOnly)
                .validate_capabilities(&supported),
            Err(RequestValidationError::UnsupportedRequiredCapability(
                "provider:structured-output".into()
            ))
        );
    }

    #[test]
    fn declared_fallback_is_observable() {
        let decisions = request(
            FeatureRequirement::AllowFallback,
            FallbackPolicy::DeclaredOnly,
        )
        .validate_capabilities(&ProviderCapabilities::default())
        .unwrap();
        assert_eq!(decisions[0].resolution, CapabilityResolution::Fallback);
    }
}
