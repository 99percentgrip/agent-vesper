use crate::{error, provider_id};
use std::sync::Arc;
use vesper_domain::{BoundedString, ExtensionMap, ModelId, QualifiedModelId, SafeMessage};
use vesper_provider::*;

pub const DEFAULT_MODEL: &str = "grok-4.7";

#[derive(Clone, Copy)]
struct Spec {
    id: &'static str,
    name: &'static str,
    context: u64,
    efforts: &'static [&'static str],
    default_effort: &'static str,
    tools: bool,
    batch: bool,
    multi_agent: bool,
    aliases: &'static [&'static str],
}
const MODELS: &[Spec] = &[
    Spec {
        id: "grok-4.7",
        name: "Grok 4.7",
        context: 500_000,
        efforts: &["low", "medium", "high", "xhigh"],
        default_effort: "high",
        tools: true,
        batch: false,
        multi_agent: false,
        aliases: &["grok-4.7-latest"],
    },
    Spec {
        id: "grok-4.6",
        name: "Grok 4.6",
        context: 500_000,
        efforts: &["low", "medium", "high", "xhigh"],
        default_effort: "high",
        tools: true,
        batch: false,
        multi_agent: false,
        aliases: &["grok-4.6-latest"],
    },
    Spec {
        id: "grok-4.5",
        name: "Grok 4.5",
        context: 500_000,
        efforts: &["low", "medium", "high"],
        default_effort: "high",
        tools: true,
        batch: false,
        multi_agent: false,
        aliases: &["grok-4.5-latest", "grok-build-latest"],
    },
    Spec {
        id: "grok-4.3",
        name: "Grok 4.3",
        context: 1_000_000,
        efforts: &["none", "low", "medium", "high"],
        default_effort: "low",
        tools: true,
        batch: true,
        multi_agent: false,
        aliases: &["grok-4.3-latest"],
    },
    Spec {
        id: "grok-4.20-0309-reasoning",
        name: "Grok 4.20 Reasoning",
        context: 1_000_000,
        efforts: &["high"],
        default_effort: "high",
        tools: true,
        batch: true,
        multi_agent: false,
        aliases: &[
            "grok-4.20",
            "grok-4.20-reasoning",
            "grok-4.20-reasoning-latest",
            "grok-4.20-0309",
            "grok-4.20-beta-0309-reasoning",
            "grok-4.20-beta",
            "grok-4.20-beta-0309",
            "grok-4.20-beta-latest",
            "grok-4.20-beta-latest-reasoning",
            "grok-4.20-beta-reasoning",
            "grok-4.20-experimental-beta-0304-reasoning",
            "grok-4.20-experimental-beta-0304",
            "grok-4.20-experimental-beta-reasoning-latest",
            "grok-4.20-experimental-beta-latest",
            "grok-4.20-reasoning-gv2",
        ],
    },
    Spec {
        id: "grok-4.20-0309-non-reasoning",
        name: "Grok 4.20 Non-Reasoning",
        context: 1_000_000,
        efforts: &["none"],
        default_effort: "none",
        tools: true,
        batch: true,
        multi_agent: false,
        aliases: &[
            "grok-4.20-non-reasoning",
            "grok-4.20-non-reasoning-latest",
            "grok-4.20-beta-non-reasoning",
            "grok-4.20-beta-latest-non-reasoning",
            "grok-4.20-experimental-beta-0304-non-reasoning",
            "grok-4.20-experimental-beta-non-reasoning-latest",
            "grok-4.20-beta-0309-non-reasoning",
            "grok-4.20-non-reasoning-gv2",
        ],
    },
    Spec {
        id: "grok-4.20-multi-agent-0309",
        name: "Grok 4.20 Multi-Agent Beta",
        context: 1_000_000,
        efforts: &["low", "medium", "high", "xhigh"],
        default_effort: "high",
        tools: false,
        batch: true,
        multi_agent: true,
        aliases: &[
            "grok-4.20-multi-agent",
            "grok-4.20-multi-agent-latest",
            "grok-4.20-multi-agent-beta-latest",
            "grok-4.20-multi-agent-experimental-beta-0304",
            "grok-4.20-multi-agent-experimental-beta-latest",
            "grok-4.20-multi-agent-beta-0309",
        ],
    },
    Spec {
        id: "grok-build-0.1",
        name: "Grok Build 0.1",
        context: 256_000,
        efforts: &["high"],
        default_effort: "high",
        tools: true,
        batch: false,
        multi_agent: false,
        aliases: &[],
    },
];

const RETIRED_REDIRECTS: &[&str] = &[
    "grok-4-1-fast-reasoning",
    "grok-4-1-fast-non-reasoning",
    "grok-4-fast-reasoning",
    "grok-4-fast-non-reasoning",
    "grok-4-0709",
    "grok-code-fast-1",
    "grok-code-fast",
    "grok-code-fast-1-0825",
    "grok-3",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogIdentity {
    Canonical(&'static str),
    MovingAlias(&'static str),
    FixedAlias(&'static str),
    RetiredRedirect,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct XaiCatalog;
impl XaiCatalog {
    pub fn snapshot() -> ModelCatalogSnapshot {
        ModelCatalogSnapshot {
            models: MODELS.iter().copied().map(descriptor).collect(),
            provenance: ModelCatalogProvenance::Static,
            expires_at_unix_ms: None,
        }
    }
    pub fn find(id: &str) -> Option<ModelDescriptor> {
        MODELS
            .iter()
            .copied()
            .find(|spec| spec.id == id)
            .map(descriptor)
    }
    pub fn resolve_current_alias(id: &str) -> Option<&'static str> {
        match Self::classify_identity(id) {
            CatalogIdentity::Canonical(id)
            | CatalogIdentity::MovingAlias(id)
            | CatalogIdentity::FixedAlias(id) => Some(id),
            CatalogIdentity::RetiredRedirect | CatalogIdentity::Unknown => None,
        }
    }
    pub fn classify_identity(id: &str) -> CatalogIdentity {
        if RETIRED_REDIRECTS.contains(&id) {
            return CatalogIdentity::RetiredRedirect;
        }
        let Some(spec) = MODELS
            .iter()
            .find(|spec| spec.id == id || spec.aliases.contains(&id))
        else {
            return CatalogIdentity::Unknown;
        };
        if spec.id == id {
            CatalogIdentity::Canonical(spec.id)
        } else if id.contains("0309") || id.contains("0304") || id.ends_with("-gv2") {
            CatalogIdentity::FixedAlias(spec.id)
        } else {
            CatalogIdentity::MovingAlias(spec.id)
        }
    }
    pub fn reasoning_levels(id: &str) -> Vec<&'static str> {
        MODELS
            .iter()
            .find(|spec| spec.id == id)
            .map(|spec| spec.efforts.to_vec())
            .unwrap_or_default()
    }
    pub fn default_effort(id: &str) -> Option<&'static str> {
        MODELS
            .iter()
            .find(|spec| spec.id == id)
            .map(|spec| spec.default_effort)
    }
    pub fn context_tokens(id: &str) -> Option<u64> {
        MODELS
            .iter()
            .find(|spec| spec.id == id)
            .map(|spec| spec.context)
    }
    pub fn supports_client_tools(id: &str) -> bool {
        MODELS.iter().any(|spec| spec.id == id && spec.tools)
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
fn descriptor(spec: Spec) -> ModelDescriptor {
    let mut metadata = ExtensionMap::default();
    metadata
        .insert(
            "xai:default-reasoning-effort",
            serde_json::json!(spec.default_effort),
        )
        .expect("static");
    metadata
        .insert("xai:aliases", serde_json::json!(spec.aliases))
        .expect("static");
    metadata
        .insert("xai:batch", serde_json::json!(spec.batch))
        .expect("static");
    if spec.multi_agent {
        metadata
            .insert(
                "xai:reasoning-control-semantics",
                serde_json::json!("agent-count"),
            )
            .expect("static");
        metadata
            .insert("xai:beta", serde_json::json!(true))
            .expect("static");
    }
    if spec.id == "grok-4.3" {
        metadata
            .insert(
                "xai:reasoning-evidence-policy",
                serde_json::json!("xhigh-withheld-general-guide-conflicts-with-model-detail"),
            )
            .expect("static");
    }
    if spec.id == "grok-4.5" {
        metadata
            .insert(
                "xai:reasoning-evidence-policy",
                serde_json::json!("xhigh-withheld-provider-aliases-to-high"),
            )
            .expect("static");
    }
    let tools = if spec.tools {
        native(ToolCapability {
            schema_dialect: "xai.responses.function".into(),
            choice_modes: vec![
                "auto".into(),
                "none".into(),
                "required".into(),
                "named".into(),
            ],
            parallel: true,
            streamed_arguments: true,
        })
    } else {
        unsupported(
            "xAI's dedicated multi-agent contract does not support client-side custom tools",
        )
    };
    let tool_choice = if spec.tools {
        native(ToolChoiceCapability {
            automatic: true,
            none: true,
            required: true,
            named: true,
        })
    } else {
        unsupported("Client-side custom tools are unavailable for this multi-agent model")
    };
    ModelDescriptor {
        model: QualifiedModelId {
            provider_id: provider_id(),
            model_id: ModelId::new(spec.id).expect("static"),
        },
        display_name: BoundedString::new(spec.name).expect("static"),
        metadata,
        capabilities: ProviderCapabilities {
            limits: native(ModelLimits {
                context_tokens: Some(spec.context),
                output_tokens: None,
                exact: true,
            }),
            reasoning: native(ReasoningCapability {
                effort_levels: spec.efforts.iter().map(|v| (*v).into()).collect(),
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
                media_types: vec!["image/png".into(), "image/jpeg".into()],
                maximum_items: None,
                maximum_bytes_per_item: Some(20 * 1024 * 1024),
                references: true,
                inline_data: true,
            }),
            audio: unsupported("xAI reasoning models do not accept audio through this adapter"),
            tools,
            tool_choice,
            parallel_tool_calls: if spec.tools {
                native(())
            } else {
                unsupported("Client-side custom tools are unavailable")
            },
            streamed_tool_arguments: if spec.tools {
                native(())
            } else {
                unsupported("Client-side custom tools are unavailable")
            },
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
            sampling: unsupported("Sampling controls are not exposed for these reasoning requests"),
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
