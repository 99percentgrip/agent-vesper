use crate::{error, provider_id};
use std::sync::Arc;
use vesper_domain::{BoundedString, ModelId, QualifiedModelId, SafeMessage};
use vesper_provider::*;

/// PR-1's verified configuration model. Account discovery is added in PR-2.
pub const DEFAULT_MODEL: &str = "grok-4.7";
pub const REASONING_LEVELS: &[&str] = &["low", "medium", "high", "xhigh"];

#[derive(Clone, Copy, Debug, Default)]
pub struct XaiCatalog;

impl XaiCatalog {
    pub fn snapshot() -> ModelCatalogSnapshot {
        ModelCatalogSnapshot {
            models: vec![descriptor()],
            provenance: ModelCatalogProvenance::Static,
            expires_at_unix_ms: None,
        }
    }
    pub fn find(id: &str) -> Option<ModelDescriptor> {
        (id == DEFAULT_MODEL).then(descriptor)
    }
    pub fn reasoning_levels(id: &str) -> Vec<&'static str> {
        if id == DEFAULT_MODEL {
            REASONING_LEVELS.to_vec()
        } else {
            Vec::new()
        }
    }
}

impl ModelCatalog for XaiCatalog {
    fn models<'a>(
        &'a self,
        cancel: Arc<dyn CancellationSignal>,
    ) -> ProviderFuture<'a, Result<ModelCatalogSnapshot, ProviderError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                Err(error(
                    "xAI request cancelled",
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
fn descriptor() -> ModelDescriptor {
    ModelDescriptor {
        model: QualifiedModelId {
            provider_id: provider_id(),
            model_id: ModelId::new(DEFAULT_MODEL).expect("static"),
        },
        display_name: BoundedString::new("Grok 4.7").expect("static"),
        metadata: Default::default(),
        capabilities: ProviderCapabilities {
            limits: native(ModelLimits {
                context_tokens: Some(500_000),
                output_tokens: None,
                exact: false,
            }),
            reasoning: native(ReasoningCapability {
                effort_levels: REASONING_LEVELS.iter().map(|v| (*v).into()).collect(),
                visible_modes: vec!["summary".into()],
            }),
            streamed_reasoning: native(StreamedReasoningCapability {
                visible_text: false,
                summaries: true,
            }),
            preserved_reasoning: native(PreservedReasoningCapability {
                visible_blocks: false,
                opaque_records: true,
            }),
            vision: native(MediaCapability {
                media_types: vec!["image/png".into(), "image/jpeg".into(), "image/webp".into()],
                maximum_items: Some(50),
                references: true,
                inline_data: true,
            }),
            audio: unsupported("xAI reasoning models do not accept audio through this adapter"),
            tools: native(ToolCapability {
                schema_dialect: "xai.responses.function".into(),
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
                controls: vec!["prompt_cache_key".into()],
                reports_reads: true,
                reports_writes: false,
            }),
            structured_output: native(StructuredOutputCapability {
                json_mode: true,
                json_schema: true,
                grammars: vec![],
            }),
            sampling: unsupported("Sampling controls are not exposed for this reasoning model"),
            model_discovery: native(vec!["authenticated-account-models".into()]),
            authentication: native(AuthenticationCapability {
                methods: vec!["xai-api-key".into()],
                optional: false,
            }),
            quota_reporting: unsupported(
                "The verified API-key path exposes usage, not account balance",
            ),
            continuation: native(ContinuationCapability {
                strategies: vec!["encrypted-reasoning-history".into()],
                provider_maximum: None,
            }),
            process_backed: unsupported("xAI uses direct native HTTP"),
            external_runtime: unsupported("Vesper owns tools and permission checks"),
        },
    }
}
