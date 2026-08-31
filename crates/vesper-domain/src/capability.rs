use serde::{Deserialize, Serialize};

use crate::{BoundedString, QualifiedModelId, SafeMessage};

/// Maximum alternatives offered for one missing capability.
pub const MAX_MODEL_CANDIDATES: usize = 3;

/// A provider-neutral model capability required by session content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ModelRequirement {
    /// The outgoing payload contains an image with this IANA media type.
    VisionImage {
        /// Required image media type.
        media_type: BoundedString<128>,
    },
}

/// One catalog-verified same-provider model alternative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCandidate {
    /// Qualified identity of the candidate model.
    pub model: QualifiedModelId,
    /// Catalog display name.
    pub display_name: BoundedString<256>,
    /// Endpoint/API plan that must be selected first, when applicable.
    pub required_plan_change: Option<BoundedString<64>>,
    /// Bounded explanation of why the catalog says this model qualifies.
    pub why_it_qualifies: SafeMessage,
}

/// Typed fail-closed outcome returned when content exceeds model capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySuggestion {
    /// Capability required by the preserved content.
    pub requirement: ModelRequirement,
    /// Active model that cannot satisfy the requirement.
    pub current_model: QualifiedModelId,
    /// Bounded provider-neutral rejection reason.
    pub reason: SafeMessage,
    /// At most three catalog-verified same-provider alternatives.
    pub candidates: Vec<ModelCandidate>,
}

impl CapabilitySuggestion {
    /// Creates a bounded suggestion and drops accidental cross-provider entries.
    #[must_use]
    pub fn new(
        requirement: ModelRequirement,
        current_model: QualifiedModelId,
        reason: SafeMessage,
        candidates: Vec<ModelCandidate>,
    ) -> Self {
        let candidates = candidates
            .into_iter()
            .filter(|candidate| candidate.model.provider_id == current_model.provider_id)
            .take(MAX_MODEL_CANDIDATES)
            .collect();
        Self {
            requirement,
            current_model,
            reason,
            candidates,
        }
    }
}

/// Stable namespaced capability identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityId(String);

impl CapabilityId {
    /// Creates a namespaced capability identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.len() > 128 || !value.contains(':') {
            return Err("capability ID must be bounded and namespaced");
        }
        Ok(Self(value))
    }

    /// Returns the identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Caller intent for a requested feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeatureRequirement {
    /// Dispatch must fail if unavailable.
    Require,
    /// Prefer support but omission is allowed and observable.
    Prefer,
    /// A declared fallback may be used.
    AllowFallback,
}

/// One requested capability and its fallback intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRequest {
    /// Capability identity.
    pub capability: CapabilityId,
    /// Required behavior.
    pub requirement: FeatureRequirement,
    /// Explicit fallback permitted when the requirement is `AllowFallback`.
    pub fallback: Option<CapabilityFallback>,
}

/// One declared fallback whose use must be observable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityFallback {
    /// Stable fallback identity.
    pub id: CapabilityId,
    /// Safe user-visible description.
    pub description: SafeMessage,
}
