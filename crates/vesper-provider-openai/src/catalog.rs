use crate::{error, provider_id};
use std::sync::Arc;
use vesper_domain::{BoundedString, ModelId, QualifiedModelId, SafeMessage};
use vesper_provider::*;

/// Fallback configuration identifier, never evidence of account availability.
pub const DEFAULT_MODEL: &str = "gpt-5.4";
/// Shared supported effort subset, never invent GLM aliases for OpenAI.
pub const REASONING_LEVELS: &[&str] = &["low", "medium", "high", "xhigh"];
// Explicit capability evidence: official model pages plus pinned upstream
// models-manager/models.json. Never infer capabilities from model-name patterns.
const MODELS: &[(&str, &str)] = &[
    ("gpt-6-astra", "GPT-6 Astra"),
    ("gpt-5.6-sol", "GPT-5.6 Sol"),
    ("gpt-5.6-terra", "GPT-5.6 Terra"),
    ("gpt-5.6-luna", "GPT-5.6 Luna"),
    ("gpt-5.5", "GPT-5.5"),
    ("gpt-5.4", "GPT-5.4"),
    ("gpt-5.2", "GPT-5.2"),
    ("gpt-5.3-codex", "GPT-5.3 Codex"),
    ("gpt-5.3-codex-spark", "GPT-5.3 Codex Spark"),
];

/// Evidence-backed catalog with a conservative shared subscription/API budget.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpenAiCatalog;
impl OpenAiCatalog {
    pub fn snapshot() -> ModelCatalogSnapshot {
        ModelCatalogSnapshot {
            models: MODELS
                .iter()
                .map(|(id, name)| descriptor(id, name))
                .collect(),
            provenance: ModelCatalogProvenance::Static,
            expires_at_unix_ms: None,
        }
    }
    pub fn find(id: &str) -> Option<ModelDescriptor> {
        MODELS
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(id, name)| descriptor(id, name))
    }
    /// Upstream Codex's default input budget, deliberately conservative for API mode.
    pub const fn context_tokens() -> u64 {
        272_000
    }
    pub fn context_tokens_for(model: &str) -> u64 {
        if model == "gpt-5.3-codex-spark" {
            128_000
        } else {
            Self::context_tokens()
        }
    }
    pub fn supports_reasoning(model: &str, effort: &str) -> bool {
        MODELS.iter().any(|(id, _)| *id == model) && Self::reasoning_levels(model).contains(&effort)
    }
    pub fn reasoning_levels_for(
        model: &str,
        mode: crate::auth::AuthenticationMode,
    ) -> Vec<&'static str> {
        let mut levels = Self::reasoning_levels(model);
        if mode == crate::auth::AuthenticationMode::ApiKey
            && !levels.is_empty()
            && !matches!(
                model,
                "gpt-6-astra" | "gpt-5.3-codex" | "gpt-5.3-codex-spark"
            )
        {
            levels.insert(0, "none");
        }
        levels
    }
    /// Literal Responses efforts, not Codex's host-owned ultra delegation mode.
    pub fn reasoning_levels(model: &str) -> Vec<&'static str> {
        if !MODELS.iter().any(|(id, _)| *id == model) {
            return Vec::new();
        }
        let mut levels = REASONING_LEVELS.to_vec();
        if matches!(
            model,
            "gpt-6-astra" | "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna"
        ) {
            levels.push("max");
        }
        levels
    }
}
impl ModelCatalog for OpenAiCatalog {
    fn models<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ModelCatalogSnapshot, ProviderError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                Err(error(
                    "OpenAI request cancelled",
                    vesper_domain::ErrorCategory::Cancellation,
                    false,
                ))
            } else {
                Ok(Self::snapshot())
            }
        })
    }
}
fn native<T>(details: T) -> SupportLevel<T> {
    SupportLevel::Native { details }
}
fn unsupported<T>(reason: &str) -> SupportLevel<T> {
    SupportLevel::Unsupported {
        reason: SafeMessage::new(reason).expect("static"),
    }
}
fn descriptor(id: &str, name: &str) -> ModelDescriptor {
    let efforts = OpenAiCatalog::reasoning_levels(id)
        .iter()
        .map(|s| s.to_string())
        .collect();
    ModelDescriptor {
        model: QualifiedModelId {
            provider_id: provider_id(),
            model_id: ModelId::new(id).expect("static"),
        },
        display_name: BoundedString::new(name).expect("static"),
        metadata: Default::default(),
        capabilities: ProviderCapabilities {
            limits: native(ModelLimits {
                context_tokens: Some(OpenAiCatalog::context_tokens_for(id)),
                output_tokens: Some(128_000),
                exact: false,
            }),
            reasoning: native(ReasoningCapability {
                effort_levels: efforts,
                visible_modes: if id == "gpt-5.3-codex-spark" {
                    vec![]
                } else {
                    vec!["summary".into()]
                },
            }),
            streamed_reasoning: native(StreamedReasoningCapability {
                visible_text: false,
                summaries: id != "gpt-5.3-codex-spark",
            }),
            preserved_reasoning: native(PreservedReasoningCapability {
                visible_blocks: false,
                opaque_records: true,
            }),
            vision: if id == "gpt-5.3-codex-spark" {
                unsupported("Codex Spark is text-only")
            } else {
                native(MediaCapability {
                    media_types: vec!["image/png".into(), "image/jpeg".into(), "image/webp".into()],
                    maximum_items: Some(50),
                    references: true,
                    inline_data: true,
                })
            },
            audio: unsupported("These OpenAI models do not accept audio input"),
            tools: native(ToolCapability {
                schema_dialect: "openai.responses.function".into(),
                choice_modes: vec![
                    "auto".into(),
                    "none".into(),
                    "required".into(),
                    "named".into(),
                ],
                parallel: true,
                streamed_arguments: true,
            }),
            tool_choice: native(ToolChoiceCapability {
                automatic: true,
                none: true,
                required: true,
                named: true,
            }),
            parallel_tool_calls: native(()),
            streamed_tool_arguments: native(()),
            prompt_caching: native(PromptCacheCapability {
                controls: vec![],
                reports_reads: true,
                reports_writes: false,
            }),
            structured_output: native(StructuredOutputCapability {
                json_mode: true,
                json_schema: true,
                grammars: vec![],
            }),
            sampling: unsupported("Sampling controls are not exposed for reasoning requests"),
            model_discovery: native(vec!["authenticated-account-models".into()]),
            authentication: native(AuthenticationCapability {
                methods: vec!["openai-api-key".into(), "openai-chatgpt".into()],
                optional: false,
            }),
            quota_reporting: native(vec!["chatgpt-subscription-account-windows".into()]),
            continuation: native(ContinuationCapability {
                strategies: vec!["encrypted-reasoning-history".into()],
                provider_maximum: None,
            }),
            process_backed: unsupported("OpenAI uses direct native HTTP"),
            external_runtime: unsupported("Vesper owns tools and permission checks"),
        },
    }
}
