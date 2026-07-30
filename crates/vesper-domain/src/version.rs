use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

/// Version attached to compatibility-sensitive envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// Creates a nonzero schema version.
    pub const fn new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// Returns the encoded version.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| de::Error::custom("schema version must be nonzero"))
    }
}

/// Contract family whose version is being validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContractFamily {
    /// Shared domain DTOs.
    Domain,
    /// Runtime commands.
    Command,
    /// Runtime events.
    Event,
    /// Frozen compatibility records.
    Compatibility,
    /// Provider-owned extension envelopes.
    ProviderExtension,
    /// Frontend-owned extension envelopes.
    FrontendExtension,
}

/// Explicit incompatibility instead of silent best-effort decoding.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unsupported {family:?} schema version {received}; supported version is {supported}")]
pub struct VersionCompatibilityError {
    /// Contract family.
    pub family: ContractFamily,
    /// Received version.
    pub received: u32,
    /// Current supported version.
    pub supported: u32,
}

macro_rules! contract_version {
    ($name:ident, $family:expr) => {
        #[doc = "Version for a compatibility-sensitive contract envelope."]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(SchemaVersion);

        impl $name {
            /// Current Stage 2 version.
            pub const CURRENT: Self = Self(SchemaVersion(1));

            /// Validates an encoded version.
            pub fn supported(value: u32) -> Result<Self, VersionCompatibilityError> {
                if value == 1 {
                    Ok(Self::CURRENT)
                } else {
                    Err(VersionCompatibilityError {
                        family: $family,
                        received: value,
                        supported: 1,
                    })
                }
            }

            /// Returns the encoded version.
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0.get()
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = u32::deserialize(deserializer)?;
                Self::supported(value).map_err(de::Error::custom)
            }
        }
    };
}

contract_version!(DomainSchemaVersion, ContractFamily::Domain);
contract_version!(CommandSchemaVersion, ContractFamily::Command);
contract_version!(EventSchemaVersion, ContractFamily::Event);
contract_version!(CompatibilityRecordVersion, ContractFamily::Compatibility);

/// Versioned opaque metadata associated with one external namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionedExtensionEnvelope {
    /// Namespace owner, such as `provider.example` or `frontend.acp`.
    pub namespace: crate::ExtensionNamespace,
    /// Namespace-defined schema version.
    pub version: SchemaVersion,
    /// Bounded extension fields.
    pub values: crate::ExtensionMap,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incompatible_contract_versions_fail_during_decode() {
        assert!(serde_json::from_str::<SchemaVersion>("0").is_err());
        assert!(serde_json::from_str::<CommandSchemaVersion>("2").is_err());
        assert_eq!(
            serde_json::from_str::<EventSchemaVersion>("1").unwrap(),
            EventSchemaVersion::CURRENT
        );
    }
}
