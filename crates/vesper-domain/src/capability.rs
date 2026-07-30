use serde::{Deserialize, Serialize};

use crate::SafeMessage;

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
