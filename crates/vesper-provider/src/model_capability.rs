//! Provider-neutral model capability gate and suggestion port.

use vesper_domain::{
    BoundedString, CapabilitySuggestion, ContentPart, ConversationMessage, ModelCandidate,
    ModelRequirement, QualifiedModelId, SafeMessage,
};

use crate::{MediaCapability, ModelDescriptor, SupportLevel, ToolCapability};

const MAX_REASON_CHARS: usize = 200;
const MAX_SCANNED_MESSAGES: usize = 256;
const MAX_SCANNED_PARTS: usize = 1_024;

/// Bounded reason why one model cannot satisfy session content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDenial {
    reason: String,
}

impl CapabilityDenial {
    /// Creates a denial while enforcing the shared reason bound.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into().chars().take(MAX_REASON_CHARS).collect(),
        }
    }

    /// Bounded provider-neutral reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl std::fmt::Display for CapabilityDenial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

/// Pure fail-closed view over one provider catalog snapshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelCapabilityIndex {
    models: Vec<ModelDescriptor>,
}

impl ModelCapabilityIndex {
    /// Builds an index from catalog descriptors.
    #[must_use]
    pub fn from_descriptors(models: Vec<ModelDescriptor>) -> Self {
        Self { models }
    }

    /// Empty, fail-closed index.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Number of catalog models.
    #[must_use]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Whether no catalog models are known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// Catalog ids in stable order.
    #[must_use]
    pub fn model_ids(&self) -> Vec<&str> {
        self.models
            .iter()
            .map(|entry| entry.model.model_id.as_str())
            .collect()
    }

    /// Whether the catalog contains a model id.
    #[must_use]
    pub fn is_known(&self, model_id: &str) -> bool {
        self.find(model_id).is_some()
    }

    fn find(&self, model_id: &str) -> Option<&ModelDescriptor> {
        self.models
            .iter()
            .find(|entry| entry.model.model_id.as_str() == model_id)
    }

    fn known(&self, model_id: &str) -> Result<&ModelDescriptor, CapabilityDenial> {
        self.find(model_id).ok_or_else(|| {
            CapabilityDenial::new(format!(
                "model `{model_id}` is not in the active provider's model catalog"
            ))
        })
    }

    /// Fail-closed image support check.
    pub fn accepts_image(&self, model_id: &str, media_type: &str) -> Result<(), CapabilityDenial> {
        let descriptor = self.known(model_id)?;
        match &descriptor.capabilities.vision {
            SupportLevel::Native { details } | SupportLevel::Emulated { details, .. } => {
                if details.media_types.is_empty() {
                    Err(CapabilityDenial::new(format!(
                        "model `{model_id}` did not report accepted image types"
                    )))
                } else if details.media_types.iter().any(|value| value == media_type) {
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

    /// Tool capability when advertised.
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

    /// Whether tool support is advertised.
    #[must_use]
    pub fn supports_tools(&self, model_id: &str) -> bool {
        self.tools(model_id).is_ok()
    }

    /// Reasoning labels advertised by the model.
    #[must_use]
    pub fn reasoning_effort_levels(&self, model_id: &str) -> Vec<String> {
        self.find(model_id)
            .map_or_else(Vec::new, |entry| match &entry.capabilities.reasoning {
                SupportLevel::Native { details } | SupportLevel::Emulated { details, .. } => {
                    details.effort_levels.clone()
                }
                SupportLevel::Unsupported { .. } | SupportLevel::Unknown => Vec::new(),
            })
    }

    /// Other tool-capable models in catalog order.
    #[must_use]
    pub fn adviser_candidates(&self, active_model: &str) -> Vec<String> {
        self.models
            .iter()
            .filter(|entry| {
                entry.model.model_id.as_str() != active_model
                    && matches!(
                        entry.capabilities.tools,
                        SupportLevel::Native { .. } | SupportLevel::Emulated { .. }
                    )
            })
            .map(|entry| entry.model.model_id.as_str().to_owned())
            .collect()
    }

    /// Exact advertised context window.
    #[must_use]
    pub fn context_window(&self, model_id: &str) -> Option<u64> {
        match &self.find(model_id)?.capabilities.limits {
            SupportLevel::Native { details } | SupportLevel::Emulated { details, .. } => {
                details.context_tokens
            }
            SupportLevel::Unsupported { .. } | SupportLevel::Unknown => None,
        }
    }

    /// Advertised image details.
    #[must_use]
    pub fn vision_details(&self, model_id: &str) -> Option<&MediaCapability> {
        match &self.find(model_id)?.capabilities.vision {
            SupportLevel::Native { details } | SupportLevel::Emulated { details, .. } => {
                Some(details)
            }
            SupportLevel::Unsupported { .. } | SupportLevel::Unknown => None,
        }
    }

    /// Catalog descriptors, for adapter policy projections.
    #[must_use]
    pub fn descriptors(&self) -> &[ModelDescriptor] {
        &self.models
    }
}

/// Session facts needed by an adapter-owned candidate resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityContext {
    /// Active adapter-defined plan label, empty when the provider has no plans.
    pub active_plan: BoundedString<64>,
}

impl Default for CapabilityContext {
    fn default() -> Self {
        Self {
            active_plan: BoundedString::new("").expect("empty plan is bounded"),
        }
    }
}

/// Adapter-owned capability alternative resolver.
pub trait CapabilityAdvisor: Send + Sync {
    /// Checks one model against a requirement.
    fn check(
        &self,
        model: &QualifiedModelId,
        requirement: &ModelRequirement,
    ) -> Result<(), CapabilityDenial>;

    /// Returns at most three catalog-verified same-provider alternatives.
    fn alternatives_for(
        &self,
        requirement: &ModelRequirement,
        current_model: &QualifiedModelId,
        context: &CapabilityContext,
    ) -> Vec<ModelCandidate>;
}

/// Provider-neutral advisor for catalogs without additional plan policy.
#[derive(Debug, Clone)]
pub struct CatalogCapabilityAdvisor {
    index: ModelCapabilityIndex,
}

impl CatalogCapabilityAdvisor {
    /// Creates an advisor from one provider catalog.
    #[must_use]
    pub fn new(index: ModelCapabilityIndex) -> Self {
        Self { index }
    }
}

impl CapabilityAdvisor for CatalogCapabilityAdvisor {
    fn check(
        &self,
        model: &QualifiedModelId,
        requirement: &ModelRequirement,
    ) -> Result<(), CapabilityDenial> {
        match requirement {
            ModelRequirement::VisionImage { media_type } => self
                .index
                .accepts_image(model.model_id.as_str(), media_type.as_str()),
        }
    }

    fn alternatives_for(
        &self,
        requirement: &ModelRequirement,
        current_model: &QualifiedModelId,
        _context: &CapabilityContext,
    ) -> Vec<ModelCandidate> {
        self.index
            .descriptors()
            .iter()
            .filter(|entry| {
                entry.model != *current_model && self.check(&entry.model, requirement).is_ok()
            })
            .take(vesper_domain::MAX_MODEL_CANDIDATES)
            .filter_map(|entry| {
                Some(ModelCandidate {
                    model: entry.model.clone(),
                    display_name: entry.display_name.clone(),
                    required_plan_change: None,
                    why_it_qualifies: SafeMessage::new(
                        "catalog reports support for the required content",
                    )
                    .ok()?,
                })
            })
            .collect()
    }
}

/// First bounded content requirement in outgoing history.
#[must_use]
pub fn requirement_for_messages(messages: &[ConversationMessage]) -> Option<ModelRequirement> {
    let mut scanned = 0usize;
    for message in messages.iter().rev().take(MAX_SCANNED_MESSAGES).rev() {
        for part in &message.content {
            if scanned >= MAX_SCANNED_PARTS {
                return None;
            }
            scanned += 1;
            if let ContentPart::Image(image) = part {
                let media_type = BoundedString::new(image.media_type.clone()).ok()?;
                return Some(ModelRequirement::VisionImage { media_type });
            }
        }
    }
    None
}

/// Evaluates all bounded outgoing content and builds one typed suggestion.
pub fn gate_messages(
    messages: &[ConversationMessage],
    current_model: &QualifiedModelId,
    advisor: &dyn CapabilityAdvisor,
    context: &CapabilityContext,
) -> Result<(), CapabilitySuggestion> {
    let Some(requirement) = requirement_for_messages(messages) else {
        return Ok(());
    };
    advisor
        .check(current_model, &requirement)
        .map_err(|denial| {
            CapabilitySuggestion::new(
                requirement.clone(),
                current_model.clone(),
                SafeMessage::new(denial.reason()).unwrap_or_else(|_| {
                    SafeMessage::new("active model cannot satisfy session content")
                        .expect("static safe message")
                }),
                advisor.alternatives_for(&requirement, current_model, context),
            )
        })
}

/// Builds the common typed outcome for an already-classified requirement.
#[must_use]
pub fn suggestion_for_requirement(
    requirement: ModelRequirement,
    current_model: &QualifiedModelId,
    advisor: &dyn CapabilityAdvisor,
    context: &CapabilityContext,
) -> CapabilitySuggestion {
    let reason = advisor
        .check(current_model, &requirement)
        .err()
        .map_or_else(
            || {
                SafeMessage::new("the provider rejected this content for the active model")
                    .expect("static safe message")
            },
            |denial| {
                SafeMessage::new(denial.reason()).unwrap_or_else(|_| {
                    SafeMessage::new("the active model cannot accept this content")
                        .expect("static safe message")
                })
            },
        );
    CapabilitySuggestion::new(
        requirement.clone(),
        current_model.clone(),
        reason,
        advisor.alternatives_for(&requirement, current_model, context),
    )
}

#[cfg(test)]
mod tests {
    use vesper_domain::{
        ContentText, ExtensionMap, ImageDescriptor, MediaSource, MessageId, MessageRole, ModelId,
        ProviderId,
    };

    use super::*;
    use crate::ProviderCapabilities;

    fn model(id: &str, vision: SupportLevel<MediaCapability>) -> ModelDescriptor {
        ModelDescriptor {
            model: QualifiedModelId {
                provider_id: ProviderId::new("fake").unwrap(),
                model_id: ModelId::new(id).unwrap(),
            },
            display_name: BoundedString::new(id).unwrap(),
            capabilities: ProviderCapabilities {
                vision,
                ..ProviderCapabilities::default()
            },
            metadata: ExtensionMap::default(),
        }
    }

    fn native_vision() -> SupportLevel<MediaCapability> {
        SupportLevel::Native {
            details: MediaCapability {
                media_types: vec!["image/png".into()],
                maximum_items: Some(8),
                references: false,
                inline_data: true,
            },
        }
    }

    fn image_message() -> ConversationMessage {
        ConversationMessage {
            id: MessageId::new("message-1").unwrap(),
            role: MessageRole::User,
            content: vec![
                ContentPart::Text(ContentText::new("inspect").unwrap()),
                ContentPart::Image(ImageDescriptor {
                    media_type: "image/png".into(),
                    source: MediaSource::Reference {
                        reference: "attachment-1".into(),
                    },
                    alt_text: None,
                }),
            ],
            extensions: ExtensionMap::default(),
        }
    }

    #[test]
    fn fake_text_model_is_named_and_capable_alternative_is_suggested() {
        let index = ModelCapabilityIndex::from_descriptors(vec![
            model("text", SupportLevel::Unknown),
            model("vision", native_vision()),
        ]);
        let advisor = CatalogCapabilityAdvisor::new(index);
        let current = QualifiedModelId {
            provider_id: ProviderId::new("fake").unwrap(),
            model_id: ModelId::new("text").unwrap(),
        };
        let outcome = gate_messages(
            &[image_message()],
            &current,
            &advisor,
            &CapabilityContext::default(),
        )
        .unwrap_err();
        assert!(outcome.reason.as_str().contains("text"));
        assert_eq!(outcome.candidates.len(), 1);
        assert_eq!(outcome.candidates[0].model.model_id.as_str(), "vision");
    }

    #[test]
    fn full_payload_scan_catches_history_images_and_is_bounded() {
        let requirement = requirement_for_messages(&[image_message()]).unwrap();
        assert!(matches!(requirement, ModelRequirement::VisionImage { .. }));
        assert!(requirement_for_messages(&[]).is_none());
    }

    #[test]
    fn candidates_are_capped_and_cross_provider_entries_are_dropped() {
        let current = QualifiedModelId {
            provider_id: ProviderId::new("fake").unwrap(),
            model_id: ModelId::new("text").unwrap(),
        };
        let candidates = (0..5)
            .map(|index| ModelCandidate {
                model: QualifiedModelId {
                    provider_id: ProviderId::new(if index == 0 { "other" } else { "fake" })
                        .unwrap(),
                    model_id: ModelId::new(format!("vision-{index}")).unwrap(),
                },
                display_name: BoundedString::new(format!("Vision {index}")).unwrap(),
                required_plan_change: None,
                why_it_qualifies: SafeMessage::new("catalog reports image support").unwrap(),
            })
            .collect();
        let suggestion = CapabilitySuggestion::new(
            ModelRequirement::VisionImage {
                media_type: BoundedString::new("image/png").unwrap(),
            },
            current,
            SafeMessage::new("unsupported").unwrap(),
            candidates,
        );
        assert_eq!(suggestion.candidates.len(), 3);
        assert!(
            suggestion
                .candidates
                .iter()
                .all(|candidate| candidate.model.provider_id.as_str() == "fake")
        );
    }
}
