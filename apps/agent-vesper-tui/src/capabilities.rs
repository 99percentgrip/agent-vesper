//! Per-model capability gating index — PRD P1
//! (`docs/provider-capability-gating-prd.md`).
//!
//! Provider-neutral, pure projection of the active provider's
//! `ModelCatalog` snapshot (`Vec<ModelDescriptor>` with per-model
//! `ProviderCapabilities`). The composition boundary (`main.rs`) builds one
//! index for the active provider at startup and on every provider switch;
//! frontend gating (image paste, mixture advisers, auxiliary/thinking value
//! validity) consults it instead of a concrete provider's catalog.
//!
//! **Fail-closed contract (PRD C-1):** a capability that is
//! `SupportLevel::Unknown`, a model missing from the index, or an empty
//! advertised media-type list denies the feature with a truthful,
//! provider-neutral reason. This mirrors `vesper_provider::resolve_support`,
//! where `Unknown` + `Require` rejects before dispatch. No query in this
//! module guesses, defaults-open, or names another provider.

use std::fmt;

use vesper_provider::{MediaCapability, ModelDescriptor, SupportLevel, ToolCapability};

/// Longest denial reason retained; reasons are user-facing status strings.
const MAX_DENIAL_REASON_CHARS: usize = 200;

/// Why a capability-gated feature is denied for one model.
///
/// The reason is bounded and provider-neutral (or an adapter-authored
/// `SafeMessage` surfaced verbatim from `SupportLevel::Unsupported`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDenial {
    reason: String,
}

impl CapabilityDenial {
    /// Creates a denial with a bounded reason (truncated on a char boundary).
    fn new(reason: impl Into<String>) -> Self {
        let reason: String = reason.into();
        let bounded = reason.chars().take(MAX_DENIAL_REASON_CHARS).collect();
        Self { reason: bounded }
    }

    /// The bounded, user-facing denial reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for CapabilityDenial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

/// Fail-closed per-model capability view over one provider's catalog.
///
/// Built from the active provider's `ModelDescriptor` list; the index owns
/// no provider identity and no I/O. Unknown models and unadvertised
/// capabilities deny (see module docs).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelCapabilityIndex {
    models: Vec<ModelDescriptor>,
}

impl ModelCapabilityIndex {
    /// Builds the index from a catalog snapshot's descriptors.
    #[must_use]
    pub fn from_descriptors(models: Vec<ModelDescriptor>) -> Self {
        Self { models }
    }

    /// An index with no models; every query denies (fail-closed).
    #[must_use]
    pub fn empty() -> Self {
        Self { models: Vec::new() }
    }

    /// Number of models in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Whether the index holds no models.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// Catalog model ids in snapshot order.
    #[must_use]
    pub fn model_ids(&self) -> Vec<&str> {
        self.models
            .iter()
            .map(|descriptor| descriptor.model.model_id.as_str())
            .collect()
    }

    /// Whether `model_id` appears in the active provider's catalog.
    #[must_use]
    pub fn is_known(&self, model_id: &str) -> bool {
        self.find(model_id).is_some()
    }

    fn find(&self, model_id: &str) -> Option<&ModelDescriptor> {
        self.models
            .iter()
            .find(|descriptor| descriptor.model.model_id.as_str() == model_id)
    }

    fn known(&self, model_id: &str) -> Result<&ModelDescriptor, CapabilityDenial> {
        self.find(model_id).ok_or_else(|| {
            CapabilityDenial::new(format!(
                "model `{model_id}` is not in the active provider's model catalog"
            ))
        })
    }

    /// Whether `model_id` accepts an image input of `media_type`
    /// (e.g. `image/png`). Fails closed on unknown models, `Unknown` /
    /// `Unsupported` vision levels, and empty advertised media-type lists.
    pub fn accepts_image(&self, model_id: &str, media_type: &str) -> Result<(), CapabilityDenial> {
        let descriptor = self.known(model_id)?;
        match &descriptor.capabilities.vision {
            SupportLevel::Native { details } | SupportLevel::Emulated { details, .. } => {
                if details.media_types.is_empty() {
                    Err(CapabilityDenial::new(format!(
                        "model `{model_id}` did not report accepted image types"
                    )))
                } else if details
                    .media_types
                    .iter()
                    .any(|accepted| accepted == media_type)
                {
                    Ok(())
                } else {
                    Err(CapabilityDenial::new(format!(
                        "model `{model_id}` does not accept `{media_type}` images"
                    )))
                }
            }
            SupportLevel::Unsupported { reason } => Err(CapabilityDenial::new(reason.as_str())),
            SupportLevel::Unknown => Err(CapabilityDenial::new(format!(
                "model `{model_id}` did not report image support"
            ))),
        }
    }

    /// The model's tool capability, when advertised as native or emulated.
    /// Fails closed on unknown models and unadvertised tool support.
    pub fn tools(&self, model_id: &str) -> Result<&ToolCapability, CapabilityDenial> {
        let descriptor = self.known(model_id)?;
        match &descriptor.capabilities.tools {
            SupportLevel::Native { details } | SupportLevel::Emulated { details, .. } => {
                Ok(details)
            }
            SupportLevel::Unsupported { reason } => Err(CapabilityDenial::new(reason.as_str())),
            SupportLevel::Unknown => Err(CapabilityDenial::new(format!(
                "model `{model_id}` did not report tool support"
            ))),
        }
    }

    /// Whether the model advertises native or emulated tool support.
    #[must_use]
    pub fn supports_tools(&self, model_id: &str) -> bool {
        self.tools(model_id).is_ok()
    }

    /// Advertised reasoning effort labels for the model (empty when the
    /// model does not advertise reasoning — never invented).
    #[must_use]
    pub fn reasoning_effort_levels(&self, model_id: &str) -> Vec<String> {
        let Ok(descriptor) = self.known(model_id) else {
            return Vec::new();
        };
        match &descriptor.capabilities.reasoning {
            SupportLevel::Native { details } | SupportLevel::Emulated { details, .. } => {
                details.effort_levels.clone()
            }
            SupportLevel::Unsupported { .. } | SupportLevel::Unknown => Vec::new(),
        }
    }

    /// Catalog models eligible as auxiliary agents (for mixture-of-agents
    /// advisers): every *other* model that advertises tool support, in
    /// catalog order. Provider-specific narrowing (for example excluding
    /// vision models from text-adviser duty) is applied by the active
    /// provider's `SuperpowerPolicy` at the caller — this index stays purely
    /// capability-driven.
    #[must_use]
    pub fn adviser_candidates(&self, active_model: &str) -> Vec<String> {
        self.models
            .iter()
            .filter(|descriptor| {
                let id = descriptor.model.model_id.as_str();
                id != active_model
                    && matches!(
                        descriptor.capabilities.tools,
                        SupportLevel::Native { .. } | SupportLevel::Emulated { .. }
                    )
            })
            .map(|descriptor| descriptor.model.model_id.as_str().to_owned())
            .collect()
    }

    /// The model's advertised context-window limit in tokens, when the
    /// provider publishes exact limits (never invented).
    #[must_use]
    pub fn context_window(&self, model_id: &str) -> Option<u64> {
        let descriptor = self.find(model_id)?;
        match &descriptor.capabilities.limits {
            SupportLevel::Native { details } => details.context_tokens,
            SupportLevel::Emulated { details, .. } => details.context_tokens,
            SupportLevel::Unsupported { .. } | SupportLevel::Unknown => None,
        }
    }

    /// The media capability details when the model advertises image input.
    #[must_use]
    pub fn vision_details(&self, model_id: &str) -> Option<&MediaCapability> {
        let descriptor = self.find(model_id)?;
        match &descriptor.capabilities.vision {
            SupportLevel::Native { details } | SupportLevel::Emulated { details, .. } => {
                Some(details)
            }
            SupportLevel::Unsupported { .. } | SupportLevel::Unknown => None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! PRD P1 fail-closed matrix: Native accepts, Unsupported surfaces the
    //! adapter reason verbatim, Unknown/missing/empty deny with neutral text.

    use super::*;
    use vesper_domain::{BoundedString, ModelId, ProviderId, QualifiedModelId};
    use vesper_provider::ProviderCapabilities;

    fn provider_id() -> ProviderId {
        ProviderId::new("test").expect("static provider id")
    }

    fn descriptor(model_id: &str, capabilities: ProviderCapabilities) -> ModelDescriptor {
        ModelDescriptor {
            model: QualifiedModelId {
                provider_id: provider_id(),
                model_id: ModelId::new(model_id).expect("bounded model id"),
            },
            display_name: BoundedString::new(model_id).expect("bounded display"),
            capabilities,
            metadata: vesper_domain::ExtensionMap::default(),
        }
    }

    fn vision_media(types: &[&str]) -> SupportLevel<MediaCapability> {
        SupportLevel::Native {
            details: MediaCapability {
                media_types: types.iter().map(ToString::to_string).collect(),
                maximum_items: None,
                references: false,
                inline_data: true,
            },
        }
    }

    fn native_tools() -> SupportLevel<ToolCapability> {
        SupportLevel::Native {
            details: ToolCapability {
                schema_dialect: "test.tools.v1".into(),
                choice_modes: vec!["auto".into()],
                parallel: false,
                streamed_arguments: false,
            },
        }
    }

    #[test]
    fn accepts_image_ok_for_advertised_media_type() {
        let index = ModelCapabilityIndex::from_descriptors(vec![descriptor(
            "vision-model",
            ProviderCapabilities {
                vision: vision_media(&["image/png", "image/jpeg"]),
                ..ProviderCapabilities::default()
            },
        )]);
        assert!(index.accepts_image("vision-model", "image/png").is_ok());
        assert!(index.accepts_image("vision-model", "image/jpeg").is_ok());
    }

    #[test]
    fn accepts_image_denies_unlisted_media_type() {
        let index = ModelCapabilityIndex::from_descriptors(vec![descriptor(
            "vision-model",
            ProviderCapabilities {
                vision: vision_media(&["image/png"]),
                ..ProviderCapabilities::default()
            },
        )]);
        let denial = index
            .accepts_image("vision-model", "image/webp")
            .expect_err("unlisted media type must deny");
        assert_eq!(
            denial.reason(),
            "model `vision-model` does not accept `image/webp` images"
        );
    }

    #[test]
    fn accepts_image_surfaces_unsupported_reason_verbatim() {
        let index = ModelCapabilityIndex::from_descriptors(vec![descriptor(
            "text-model",
            ProviderCapabilities {
                vision: SupportLevel::Unsupported {
                    reason: BoundedString::new("model does not accept image input")
                        .expect("bounded"),
                },
                ..ProviderCapabilities::default()
            },
        )]);
        let denial = index
            .accepts_image("text-model", "image/png")
            .expect_err("unsupported vision must deny");
        assert_eq!(denial.reason(), "model does not accept image input");
    }

    #[test]
    fn accepts_image_fails_closed_on_unknown_and_missing() {
        let index = ModelCapabilityIndex::from_descriptors(vec![descriptor(
            "mum-model",
            ProviderCapabilities::default(),
        )]);
        let unknown = index
            .accepts_image("mum-model", "image/png")
            .expect_err("Unknown vision must deny");
        assert_eq!(
            unknown.reason(),
            "model `mum-model` did not report image support"
        );
        let missing = index
            .accepts_image("not-in-catalog", "image/png")
            .expect_err("missing model must deny");
        assert_eq!(
            missing.reason(),
            "model `not-in-catalog` is not in the active provider's model catalog"
        );
    }

    #[test]
    fn accepts_image_fails_closed_on_empty_media_type_list() {
        let index = ModelCapabilityIndex::from_descriptors(vec![descriptor(
            "odd-vision",
            ProviderCapabilities {
                vision: vision_media(&[]),
                ..ProviderCapabilities::default()
            },
        )]);
        let denial = index
            .accepts_image("odd-vision", "image/png")
            .expect_err("empty advertised media types must deny");
        assert_eq!(
            denial.reason(),
            "model `odd-vision` did not report accepted image types"
        );
    }

    #[test]
    fn tools_gate_distinguishes_native_unsupported_and_unknown() {
        let index = ModelCapabilityIndex::from_descriptors(vec![
            descriptor(
                "tool-model",
                ProviderCapabilities {
                    tools: native_tools(),
                    ..ProviderCapabilities::default()
                },
            ),
            descriptor(
                "no-tools",
                ProviderCapabilities {
                    tools: SupportLevel::Unsupported {
                        reason: BoundedString::new("hosted tools not supported").expect("bounded"),
                    },
                    ..ProviderCapabilities::default()
                },
            ),
            descriptor("mum-tools", ProviderCapabilities::default()),
        ]);
        assert!(index.tools("tool-model").is_ok());
        assert!(index.supports_tools("tool-model"));
        let denial = index
            .tools("no-tools")
            .expect_err("unsupported tools must deny");
        assert_eq!(denial.reason(), "hosted tools not supported");
        let unknown = index
            .tools("mum-tools")
            .expect_err("Unknown tools must deny");
        assert_eq!(
            unknown.reason(),
            "model `mum-tools` did not report tool support"
        );
        assert!(!index.supports_tools("no-tools"));
    }

    #[test]
    fn adviser_candidates_exclude_active_and_non_tool_models_in_order() {
        let index = ModelCapabilityIndex::from_descriptors(vec![
            descriptor(
                "alpha",
                ProviderCapabilities {
                    tools: native_tools(),
                    ..ProviderCapabilities::default()
                },
            ),
            descriptor("beta", ProviderCapabilities::default()),
            descriptor(
                "gamma",
                ProviderCapabilities {
                    tools: native_tools(),
                    ..ProviderCapabilities::default()
                },
            ),
        ]);
        assert_eq!(index.adviser_candidates("gamma"), vec!["alpha".to_owned()]);
        assert_eq!(
            index.adviser_candidates("beta"),
            vec!["alpha".to_owned(), "gamma".to_owned()]
        );
        // The active model is never its own adviser even when tool-capable.
        assert!(
            !index
                .adviser_candidates("alpha")
                .contains(&"alpha".to_owned())
        );
    }

    #[test]
    fn reasoning_effort_levels_follow_advertisement() {
        let deep = ProviderCapabilities {
            reasoning: SupportLevel::Native {
                details: vesper_provider::ReasoningCapability {
                    effort_levels: vec![
                        "disabled".into(),
                        "enabled".into(),
                        "high".into(),
                        "max".into(),
                    ],
                    visible_modes: vec!["provider-visible".into()],
                },
            },
            ..ProviderCapabilities::default()
        };
        let base = ProviderCapabilities {
            reasoning: SupportLevel::Native {
                details: vesper_provider::ReasoningCapability {
                    effort_levels: vec!["disabled".into(), "enabled".into()],
                    visible_modes: Vec::new(),
                },
            },
            ..ProviderCapabilities::default()
        };
        let index = ModelCapabilityIndex::from_descriptors(vec![
            descriptor("deep", deep),
            descriptor("base", base),
            descriptor("none", ProviderCapabilities::default()),
        ]);
        assert_eq!(
            index.reasoning_effort_levels("deep"),
            vec!["disabled", "enabled", "high", "max"]
        );
        assert_eq!(
            index.reasoning_effort_levels("base"),
            vec!["disabled", "enabled"]
        );
        assert!(index.reasoning_effort_levels("none").is_empty());
        assert!(index.reasoning_effort_levels("missing").is_empty());
    }

    #[test]
    fn empty_index_denies_everything() {
        let index = ModelCapabilityIndex::empty();
        assert!(index.is_empty());
        assert!(!index.is_known("any"));
        assert!(index.accepts_image("any", "image/png").is_err());
        assert!(index.tools("any").is_err());
        assert!(index.adviser_candidates("any").is_empty());
    }

    #[test]
    fn denial_reasons_are_bounded_on_char_boundaries() {
        let long = "x".repeat(500);
        let index = ModelCapabilityIndex::from_descriptors(vec![descriptor(
            "long",
            ProviderCapabilities {
                vision: SupportLevel::Unsupported {
                    reason: BoundedString::new(long).expect("bounded by SafeMessage"),
                },
                ..ProviderCapabilities::default()
            },
        )]);
        let denial = index
            .accepts_image("long", "image/png")
            .expect_err("unsupported");
        assert!(denial.reason().chars().count() <= MAX_DENIAL_REASON_CHARS);
    }

    #[test]
    fn context_window_reads_advertised_limits_only() {
        let index = ModelCapabilityIndex::from_descriptors(vec![descriptor(
            "limited",
            ProviderCapabilities {
                limits: SupportLevel::Native {
                    details: vesper_provider::ModelLimits {
                        context_tokens: Some(131_072),
                        output_tokens: Some(16_384),
                        exact: true,
                    },
                },
                ..ProviderCapabilities::default()
            },
        )]);
        assert_eq!(index.context_window("limited"), Some(131_072));
        assert_eq!(index.context_window("missing"), None);
        let unadvertised = ModelCapabilityIndex::from_descriptors(vec![descriptor(
            "mum",
            ProviderCapabilities::default(),
        )]);
        assert_eq!(unadvertised.context_window("mum"), None);
    }

    #[test]
    fn vision_details_expose_media_types_only_when_advertised() {
        let index = ModelCapabilityIndex::from_descriptors(vec![
            descriptor(
                "vision-model",
                ProviderCapabilities {
                    vision: vision_media(&["image/png"]),
                    ..ProviderCapabilities::default()
                },
            ),
            descriptor("text-model", ProviderCapabilities::default()),
        ]);
        assert_eq!(
            index
                .vision_details("vision-model")
                .map(|details| details.media_types.clone()),
            Some(vec!["image/png".to_owned()])
        );
        assert!(index.vision_details("text-model").is_none());
    }
}
