use std::sync::Arc;

use serde_json::json;
use vesper_domain::{BoundedString, ExtensionMap, ModelId, QualifiedModelId, SafeMessage};
use vesper_provider::{
    AuthenticationCapability, CancellationSignal, ContinuationCapability, MediaCapability,
    ModelCatalog, ModelCatalogProvenance, ModelCatalogSnapshot, ModelDescriptor, ModelLimits,
    PreservedReasoningCapability, PromptCacheCapability, ProviderCapabilities, ProviderError,
    ProviderFuture, ReasoningCapability, StreamedReasoningCapability, SupportLevel, ToolCapability,
    ToolChoiceCapability,
};

use crate::{GlmPlan, error::cancelled_error, provider_id};

#[derive(Clone, Copy)]
pub struct GlmModelInfo {
    id: &'static str,
    display: &'static str,
    description: &'static str,
    context: u64,
    output: u64,
    plans: &'static [GlmPlan],
    vision: bool,
    reasoning_levels: &'static [&'static str],
}

impl GlmModelInfo {
    #[must_use]
    pub const fn id(&self) -> &'static str {
        self.id
    }
    #[must_use]
    pub const fn display(&self) -> &'static str {
        self.display
    }
    #[must_use]
    pub const fn description(&self) -> &'static str {
        self.description
    }
    #[must_use]
    pub const fn context_tokens(&self) -> u64 {
        self.context
    }
    #[must_use]
    pub fn supports_plan(&self, plan: GlmPlan) -> bool {
        self.plans.contains(&plan)
    }
    #[must_use]
    pub const fn is_vision(&self) -> bool {
        self.vision
    }
    #[must_use]
    pub const fn reasoning_levels(&self) -> &'static [&'static str] {
        self.reasoning_levels
    }
}

const ALL_PLANS: &[GlmPlan] = &[GlmPlan::Coding, GlmPlan::Standard, GlmPlan::BigModel];
const API_PLANS: &[GlmPlan] = &[GlmPlan::Standard, GlmPlan::BigModel];
const FLASH_PLANS: &[GlmPlan] = &[GlmPlan::Coding, GlmPlan::Standard];
const STANDARD_REASONING: &[&str] = &["disabled", "enabled"];
const DEEP_REASONING: &[&str] = &["disabled", "enabled", "high", "max"];
// GLM-5.3-Flash requires thinking.type=enabled. The model guide explicitly
// recommends reasoning_effort=max, while the effort-less enabled form is also
// valid. Do not advertise the unsupported disabled mode or undocumented high.
const FLASH_REASONING: &[&str] = &["enabled", "max"];

const MODELS: &[GlmModelInfo] = &[
    GlmModelInfo {
        id: "glm-5.3",
        display: "GLM-5.3 (Flagship)",
        description: "Latest flagship — advanced complex software engineering, agent tasks, and emergent cybersecurity",
        context: 1_000_000,
        output: 128_000,
        plans: ALL_PLANS,
        vision: false,
        reasoning_levels: DEEP_REASONING,
    },
    GlmModelInfo {
        id: "glm-5.3-flash",
        display: "GLM-5.3-Flash (Multimodal)",
        description: "Fast native multimodal coding and agent model with visual understanding",
        context: 1_000_000,
        output: 128_000,
        plans: FLASH_PLANS,
        vision: true,
        reasoning_levels: FLASH_REASONING,
    },
    GlmModelInfo {
        id: "glm-5.2",
        display: "GLM-5.2",
        description: "Flagship reasoning, coding, and long-horizon agentic tasks",
        context: 1_000_000,
        output: 128_000,
        plans: ALL_PLANS,
        vision: false,
        reasoning_levels: DEEP_REASONING,
    },
    GlmModelInfo {
        id: "glm-5-turbo",
        display: "GLM-5-Turbo",
        description: "Flagship model optimized for speed",
        context: 200_000,
        output: 128_000,
        plans: ALL_PLANS,
        vision: false,
        reasoning_levels: STANDARD_REASONING,
    },
    GlmModelInfo {
        id: "glm-4.7",
        display: "GLM-4.7",
        description: "Balanced model for daily development and routine tasks",
        context: 200_000,
        output: 128_000,
        plans: ALL_PLANS,
        vision: false,
        reasoning_levels: STANDARD_REASONING,
    },
    GlmModelInfo {
        id: "glm-5v-turbo",
        display: "GLM-5V-Turbo (Vision Coding)",
        description: "Multimodal coding model for screenshots, video, UI, and agent workflows",
        context: 200_000,
        output: 128_000,
        plans: API_PLANS,
        vision: true,
        reasoning_levels: STANDARD_REASONING,
    },
    GlmModelInfo {
        id: "glm-4.5v",
        display: "GLM-4.5V (Vision)",
        description: "Vision-capable model for screenshots, diagrams, and charts",
        context: 65_536,
        output: 16_384,
        plans: API_PLANS,
        vision: true,
        reasoning_levels: STANDARD_REASONING,
    },
    GlmModelInfo {
        id: "glm-4.6v",
        display: "GLM-4.6V (Vision)",
        description: "Vision model with improved OCR and image understanding",
        context: 131_072,
        output: 32_768,
        plans: API_PLANS,
        vision: true,
        reasoning_levels: STANDARD_REASONING,
    },
];

/// Frozen built-in GLM model catalog.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlmCatalog;

impl GlmCatalog {
    /// Returns the adapter-owned model data used by both composition hosts.
    #[must_use]
    pub fn entries() -> &'static [GlmModelInfo] {
        MODELS
    }

    /// Returns a static catalog snapshot.
    #[must_use]
    pub fn snapshot() -> ModelCatalogSnapshot {
        ModelCatalogSnapshot {
            models: MODELS.iter().map(descriptor).collect(),
            provenance: ModelCatalogProvenance::Static,
            expires_at_unix_ms: None,
        }
    }

    /// Looks up one known model.
    #[must_use]
    pub fn find(model: &str) -> Option<ModelDescriptor> {
        MODELS
            .iter()
            .find(|entry| entry.id == model)
            .map(descriptor)
    }

    /// Returns whether a frozen model is available on an official API plan.
    #[must_use]
    pub fn supports_plan(model: &str, plan: GlmPlan) -> bool {
        model_supports_plan(model, plan)
    }

    /// Returns whether a frozen model accepts direct image input.
    #[must_use]
    pub fn is_vision_model(model: &str) -> bool {
        MODELS
            .iter()
            .find(|entry| entry.id == model)
            .is_some_and(|entry| entry.vision)
    }

    /// Returns whether a model accepts the adapter's combined reasoning mode.
    #[must_use]
    pub fn supports_reasoning_mode(model: &str, mode: &str) -> bool {
        MODELS
            .iter()
            .find(|entry| entry.id == model)
            .is_some_and(|entry| entry.reasoning_levels.contains(&mode))
    }
}

impl ModelCatalog for GlmCatalog {
    fn models<'a>(
        &'a self,
        cancellation: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ModelCatalogSnapshot, ProviderError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                Err(cancelled_error(false))
            } else {
                Ok(Self::snapshot())
            }
        })
    }
}

fn descriptor(model: &GlmModelInfo) -> ModelDescriptor {
    let mut metadata = ExtensionMap::default();
    metadata
        .insert(
            "zai:catalog",
            json!({
                "description": model.description,
                "plans": model.plans.iter().map(|plan| plan.legacy_key()).collect::<Vec<_>>(),
            }),
        )
        .expect("bounded static catalog metadata");
    let capabilities = ProviderCapabilities {
        limits: SupportLevel::Native {
            details: ModelLimits {
                context_tokens: Some(model.context),
                output_tokens: Some(model.output),
                exact: true,
            },
        },
        reasoning: SupportLevel::Native {
            details: ReasoningCapability {
                effort_levels: model
                    .reasoning_levels
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                visible_modes: vec!["provider-visible".into()],
            },
        },
        streamed_reasoning: SupportLevel::Native {
            details: StreamedReasoningCapability {
                visible_text: true,
                summaries: false,
            },
        },
        preserved_reasoning: SupportLevel::Native {
            details: PreservedReasoningCapability {
                visible_blocks: true,
                opaque_records: false,
            },
        },
        vision: if model.vision {
            SupportLevel::Native {
                details: MediaCapability {
                    media_types: vec!["image/png".into(), "image/jpeg".into(), "image/webp".into()],
                    maximum_items: None,
                    references: true,
                    inline_data: true,
                },
            }
        } else {
            unsupported("model does not accept image input")
        },
        audio: unsupported("GLM adapter does not support audio input"),
        tools: SupportLevel::Native {
            details: ToolCapability {
                schema_dialect: "zai.chat-completions.function-v1".into(),
                choice_modes: vec![
                    "auto".into(),
                    "none".into(),
                    "required".into(),
                    "named".into(),
                ],
                parallel: true,
                streamed_arguments: true,
            },
        },
        tool_choice: SupportLevel::Native {
            details: ToolChoiceCapability {
                automatic: true,
                none: true,
                required: true,
                named: true,
            },
        },
        parallel_tool_calls: SupportLevel::Native { details: () },
        streamed_tool_arguments: SupportLevel::Native { details: () },
        prompt_caching: SupportLevel::Native {
            details: PromptCacheCapability {
                controls: Vec::new(),
                reports_reads: true,
                reports_writes: false,
            },
        },
        structured_output: unsupported("structured output is not confirmed by the frozen source"),
        sampling: SupportLevel::Native {
            details: vec!["temperature".into(), "top_p".into()],
        },
        model_discovery: SupportLevel::Emulated {
            details: vec!["static-built-in".into()],
            caveat: safe("catalog is frozen and built in"),
        },
        authentication: SupportLevel::Native {
            details: AuthenticationCapability {
                methods: vec!["zai-api-key".into()],
                optional: false,
            },
        },
        quota_reporting: SupportLevel::Native {
            details: vec!["coding-plan-monitor".into()],
        },
        continuation: SupportLevel::Native {
            details: ContinuationCapability {
                strategies: vec!["replay-with-provider-message".into()],
                provider_maximum: Some(20),
            },
        },
        process_backed: unsupported("GLM uses direct HTTP transport"),
        external_runtime: unsupported("GLM does not own harness tools or permissions"),
    };
    ModelDescriptor {
        model: QualifiedModelId {
            provider_id: provider_id(),
            model_id: ModelId::new(model.id).expect("static model ID"),
        },
        display_name: BoundedString::new(model.display).expect("bounded static display"),
        capabilities,
        metadata,
    }
}

fn safe(value: &'static str) -> SafeMessage {
    SafeMessage::new(value).expect("bounded static message")
}

fn unsupported<T>(reason: &'static str) -> SupportLevel<T> {
    SupportLevel::Unsupported {
        reason: safe(reason),
    }
}

pub(crate) fn is_known_model(model: &str) -> bool {
    MODELS.iter().any(|entry| entry.id == model)
}

pub(crate) fn model_supports_plan(model: &str, plan: GlmPlan) -> bool {
    if plan == GlmPlan::Custom {
        return is_known_model(model);
    }
    MODELS
        .iter()
        .find(|entry| entry.id == model)
        .is_some_and(|entry| entry.plans.contains(&plan))
}

pub(crate) fn model_output_limit(model: &str) -> Option<u64> {
    MODELS
        .iter()
        .find(|entry| entry.id == model)
        .map(|entry| entry.output)
}

/// Returns whether a frozen model supports an extended reasoning effort level.
pub(crate) fn supports_deep_reasoning(model: &str) -> bool {
    MODELS
        .iter()
        .find(|entry| entry.id == model)
        .is_some_and(|entry| {
            entry.reasoning_levels.contains(&"high") || entry.reasoning_levels.contains(&"max")
        })
}

#[cfg(test)]
mod tests {
    use vesper_domain::ProviderId;

    use super::*;

    #[test]
    fn frozen_catalog_and_plan_eligibility_match_source() {
        let snapshot = GlmCatalog::snapshot();
        assert_eq!(snapshot.models.len(), 8);
        assert_eq!(snapshot.provenance, ModelCatalogProvenance::Static);
        assert!(model_supports_plan("glm-5.3", GlmPlan::Coding));
        assert!(model_supports_plan("glm-5.2", GlmPlan::Coding));
        assert!(model_supports_plan("glm-5.3-flash", GlmPlan::Coding));
        assert!(!model_supports_plan("glm-4.5v", GlmPlan::Coding));
        assert!(model_supports_plan("glm-4.5v", GlmPlan::Standard));
        assert_eq!(model_output_limit("glm-4.5v"), Some(16_384));
    }

    #[test]
    fn extended_reasoning_follows_per_model_capabilities() {
        assert!(supports_deep_reasoning("glm-5.3"));
        assert!(supports_deep_reasoning("glm-5.2"));
        assert!(supports_deep_reasoning("glm-5.3-flash"));
        assert!(!supports_deep_reasoning("glm-5-turbo"));
        assert!(!supports_deep_reasoning("glm-5v-turbo"));
        assert!(!supports_deep_reasoning("glm-4.7"));
    }

    #[test]
    fn flagship_displays_and_limits_match_the_lineup() {
        let flagship = GlmCatalog::find("glm-5.3").expect("glm-5.3 is registered");
        assert_eq!(flagship.display_name.as_str(), "GLM-5.3 (Flagship)");
        let legacy = GlmCatalog::find("glm-5.2").expect("glm-5.2 stays selectable");
        assert_eq!(legacy.display_name.as_str(), "GLM-5.2");
        assert_eq!(model_output_limit("glm-5.3"), Some(128_000));
        let flash = GlmCatalog::find("glm-5.3-flash").expect("flash model is registered");
        assert_eq!(model_output_limit("glm-5.3-flash"), Some(128_000));
        assert!(matches!(
            flash.capabilities.vision,
            SupportLevel::Native { .. }
        ));
        assert!(!GlmCatalog::supports_reasoning_mode(
            "glm-5.3-flash",
            "disabled"
        ));
        assert!(GlmCatalog::supports_reasoning_mode(
            "glm-5.3-flash",
            "enabled"
        ));
        assert!(GlmCatalog::supports_reasoning_mode("glm-5.3-flash", "max"));
    }

    #[test]
    fn all_models_are_provider_qualified() {
        for model in GlmCatalog::snapshot().models {
            assert_eq!(model.model.provider_id, ProviderId::new("zai").unwrap());
        }
    }
}
