use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

/// Validation error shared by opaque identifier classes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdError {
    /// Identifier is empty.
    #[error("{kind} must not be empty")]
    Empty {
        /// Identifier class.
        kind: &'static str,
    },
    /// Identifier exceeds the compatibility bound.
    #[error("{kind} exceeds {maximum} bytes")]
    TooLong {
        /// Identifier class.
        kind: &'static str,
        /// Maximum byte length.
        maximum: usize,
    },
    /// Identifier contains control characters or surrounding whitespace.
    #[error("{kind} contains invalid characters")]
    InvalidCharacters {
        /// Identifier class.
        kind: &'static str,
    },
}

fn validate(kind: &'static str, value: &str) -> Result<(), IdError> {
    if value.is_empty() {
        return Err(IdError::Empty { kind });
    }
    if value.len() > 256 {
        return Err(IdError::TooLong { kind, maximum: 256 });
    }
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(IdError::InvalidCharacters { kind });
    }
    Ok(())
}

macro_rules! opaque_id {
    ($name:ident, $kind:literal) => {
        #[doc = concat!("Opaque, validated ", $kind, " identifier.")]
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier while preserving its opaque bytes.
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                validate($kind, &value)?;
                Ok(Self(value))
            }

            /// Returns the opaque identifier text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($name))
                    .field(&self.0)
                    .finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

opaque_id!(ProviderId, "provider ID");
opaque_id!(ModelId, "model ID");
opaque_id!(EndpointId, "endpoint ID");
opaque_id!(SessionId, "session ID");
opaque_id!(TurnId, "turn ID");
opaque_id!(MessageId, "message ID");
opaque_id!(ToolCallId, "tool-call ID");
opaque_id!(ToolId, "tool ID");
opaque_id!(WorkerId, "worker ID");
opaque_id!(EventId, "event ID");
opaque_id!(ToolResultId, "tool-result ID");
opaque_id!(PlanId, "plan ID");
opaque_id!(GoalId, "goal ID");
opaque_id!(CheckpointRef, "checkpoint reference");
opaque_id!(ProviderRequestId, "provider-request ID");
opaque_id!(ProviderResponseId, "provider-response ID");
opaque_id!(CommandId, "command ID");
opaque_id!(CorrelationId, "correlation ID");
opaque_id!(RequestId, "reasoning-request ID");
opaque_id!(CandidateId, "reasoning-candidate ID");

/// Model identity qualified by its provider to prevent cross-provider ambiguity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct QualifiedModelId {
    /// Provider owner.
    pub provider_id: ProviderId,
    /// Provider-issued opaque model ID.
    pub model_id: ModelId,
}

/// Optimistic concurrency revision for mutable runtime state.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
    /// Creates a revision.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric revision.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_classes_have_stable_round_trips() {
        let provider = ProviderId::new("zai").unwrap();
        let encoded = serde_json::to_string(&provider).unwrap();
        assert_eq!(encoded, r#""zai""#);
        assert_eq!(
            serde_json::from_str::<ProviderId>(&encoded).unwrap(),
            provider
        );
    }

    #[test]
    fn identifier_classes_are_not_interchangeable() {
        let provider = ProviderId::new("same").unwrap();
        let model = ModelId::new("same").unwrap();
        assert_eq!(provider.as_str(), model.as_str());
        let _: ProviderId = provider;
        let _: ModelId = model;
    }

    #[test]
    fn invalid_identifier_is_rejected_during_deserialization() {
        assert!(serde_json::from_str::<SessionId>(r#"" bad ""#).is_err());
    }
}
