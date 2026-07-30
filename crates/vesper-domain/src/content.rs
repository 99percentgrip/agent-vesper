use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use thiserror::Error;

use crate::{ContentText, ExtensionMap, ProviderId, ToolCall, ToolResult};

/// Descriptor for inline media bytes stored outside this DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineDataDescriptor {
    /// Encoding used by the external byte carrier.
    pub encoding: String,
    /// Exact byte count before encoding.
    pub byte_length: u64,
    /// Optional integrity digest.
    pub sha256: Option<String>,
}

/// Media source without embedding unbounded bytes in generic messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MediaSource {
    /// Path, content ID, or adapter-owned reference.
    Reference {
        /// Opaque reference interpreted by an authorized adapter.
        reference: String,
    },
    /// Descriptor for bytes carried through a separate bounded channel.
    InlineDescriptor(InlineDataDescriptor),
}

/// Image content descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageDescriptor {
    /// IANA media type.
    pub media_type: String,
    /// Image data source.
    pub source: MediaSource,
    /// Optional accessible description.
    pub alt_text: Option<String>,
}

/// Audio content descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDescriptor {
    /// IANA media type.
    pub media_type: String,
    /// Audio data source.
    pub source: MediaSource,
    /// Optional duration in milliseconds.
    pub duration_ms: Option<u64>,
}

/// Provider-owned content preserved without teaching core its schema.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct OpaqueContent {
    /// Provider namespace owning the block.
    pub provider_id: ProviderId,
    /// Provider-defined block kind.
    pub kind: String,
    /// Opaque provider payload.
    pub data: OpaqueProviderData,
}

impl fmt::Debug for OpaqueContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpaqueContent")
            .field("provider_id", &self.provider_id)
            .field("kind", &self.kind)
            .field("data", &"<provider-opaque>")
            .finish()
    }
}

/// Bounded provider-owned payload with redacted `Debug`.
#[derive(Clone, PartialEq)]
pub struct OpaqueProviderData(Value);

/// Opaque provider payload exceeded the shared transport bound.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("provider-opaque content exceeds 1048576 encoded bytes")]
pub struct OpaqueProviderDataError;

impl OpaqueProviderData {
    /// Creates a bounded provider-owned value.
    pub fn new(value: Value) -> Result<Self, OpaqueProviderDataError> {
        let encoded = serde_json::to_vec(&value).map_err(|_| OpaqueProviderDataError)?;
        if encoded.len() > 1_048_576 {
            return Err(OpaqueProviderDataError);
        }
        Ok(Self(value))
    }

    /// Explicitly exposes provider-owned data to its adapter/compatibility codec.
    #[must_use]
    pub fn expose(&self) -> &Value {
        &self.0
    }
}

impl fmt::Debug for OpaqueProviderData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<provider-opaque>")
    }
}

impl Serialize for OpaqueProviderData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OpaqueProviderData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Value::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Provider-visible or provider-preserved reasoning, never hidden model chain-of-thought.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningBlock {
    /// Reasoning classification.
    pub kind: ReasoningKind,
    /// Retention instruction.
    pub retention: ReasoningRetention,
    /// Displayable text for visible reasoning/summary; absent for opaque records.
    pub text: Option<ContentText>,
    /// Provider-owned continuation material.
    pub opaque: Option<OpaqueContent>,
}

/// Reference to runtime-generated untrusted context carried outside ordinary messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedContextReference {
    /// Stable context source class.
    pub source: String,
    /// Opaque content-addressed or runtime-owned reference.
    pub reference: String,
    /// Whether adapters may expose the referenced content to a provider.
    pub provider_visible: bool,
}

/// Reasoning that a provider intentionally exposes is distinct from hidden model internals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningKind {
    /// Provider-visible reasoning text.
    ProviderVisible,
    /// Provider-produced summary intended for display.
    Summary,
    /// Opaque continuation record that is not displayable.
    OpaqueContinuation,
}

/// Retention instruction attached to exposed or opaque reasoning records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningRetention {
    /// Persist according to the active privacy policy.
    Persist,
    /// Retain only in the active session process.
    SessionOnly,
    /// Do not retain.
    Disabled,
}

/// Ordered provider-neutral message content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "kebab-case")]
pub enum ContentPart {
    /// UTF-8 text.
    Text(ContentText),
    /// Image descriptor.
    Image(ImageDescriptor),
    /// Audio descriptor.
    Audio(AudioDescriptor),
    /// Tool invocation.
    ToolCall(ToolCall),
    /// Tool result.
    ToolResult(ToolResult),
    /// Provider-visible, summarized, or opaque continuation reasoning.
    Reasoning(ReasoningBlock),
    /// Runtime-generated untrusted context reference.
    EmbeddedContext(EmbeddedContextReference),
    /// Provider-owned data preserved without interpretation.
    ProviderOpaque(OpaqueContent),
}

/// Extension metadata useful to content adapters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContentExtensions {
    /// Namespaced values.
    #[serde(default)]
    pub values: ExtensionMap,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn opaque_provider_data_round_trips_but_debug_is_redacted() {
        let data = OpaqueProviderData::new(json!({"continuation": "opaque-value"})).unwrap();
        assert!(!format!("{data:?}").contains("opaque-value"));
        let decoded: OpaqueProviderData =
            serde_json::from_value(serde_json::to_value(&data).unwrap()).unwrap();
        assert_eq!(decoded.expose(), data.expose());
    }
}
