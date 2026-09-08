use crate::{error, provider_id};
use std::sync::Arc;
use vesper_domain::{BoundedString, ModelId, QualifiedModelId, SafeMessage};
use vesper_provider::*;

/// Default available in both pinned upstream subscription and public API catalogs.
pub const DEFAULT_MODEL: &str = "gpt-5.4";
/// Shared supported effort subset, never invent GLM aliases for OpenAI.
pub const REASONING_LEVELS: &[&str] = &["low", "medium", "high", "xhigh"];
const MODELS: &[(&str, &str)] = &[("gpt-5.4", "GPT-5.4"), ("gpt-6-astra", "GPT-6 Astra")];

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
    pub fn supports_reasoning(model: &str, effort: &str) -> bool {
        Self::find(model).is_some()
            && (REASONING_LEVELS.contains(&effort) || model == "gpt-6-astra" && effort == "max")
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
    let mut efforts: Vec<String> = REASONING_LEVELS.iter().map(|s| s.to_string()).collect();
    if id == "gpt-6-astra" {
        efforts.push("max".into());
    }
    ModelDescriptor {
        model: QualifiedModelId { provider_id:provider_id(),model_id:ModelId::new(id).expect("static") },
        display_name:BoundedString::new(name).expect("static"), metadata:Default::default(),
        capabilities:ProviderCapabilities {
            limits:native(ModelLimits{context_tokens:Some(OpenAiCatalog::context_tokens()),output_tokens:Some(128_000),exact:false}),
            reasoning:native(ReasoningCapability{effort_levels:efforts,visible_modes:vec!["summary".into()]}),
            streamed_reasoning:native(StreamedReasoningCapability{visible_text:false,summaries:true}),
            preserved_reasoning:native(PreservedReasoningCapability{visible_blocks:false,opaque_records:true}),
            vision:native(MediaCapability{media_types:vec!["image/png".into(),"image/jpeg".into(),"image/webp".into()],maximum_items:Some(50),references:true,inline_data:true}),
            audio:unsupported("These OpenAI models do not accept audio input"),
            tools:native(ToolCapability{schema_dialect:"openai.responses.function".into(),choice_modes:vec!["auto".into(),"none".into(),"required".into(),"named".into()],parallel:true,streamed_arguments:true}),
            tool_choice:native(ToolChoiceCapability{automatic:true,none:true,required:true,named:true}),
            parallel_tool_calls:native(()),streamed_tool_arguments:native(()),
            prompt_caching:native(PromptCacheCapability{controls:vec![],reports_reads:true,reports_writes:false}),
            structured_output:native(StructuredOutputCapability{json_mode:true,json_schema:true,grammars:vec![]}),
            sampling:unsupported("Sampling controls are not exposed for reasoning requests"),
            model_discovery:SupportLevel::Emulated{details:vec!["static-verified-catalog".into()],caveat:SafeMessage::new("Availability depends on the account; context budget is conservative for both authentication modes").expect("static")},
            authentication:native(AuthenticationCapability{methods:vec!["openai-api-key".into(),"openai-chatgpt".into()],optional:false}),
            quota_reporting:SupportLevel::Unknown,
            continuation:native(ContinuationCapability{strategies:vec!["encrypted-reasoning-history".into()],provider_maximum:None}),
            process_backed:unsupported("OpenAI uses direct native HTTP"),external_runtime:unsupported("Vesper owns tools and permission checks"),
        },
    }
}
