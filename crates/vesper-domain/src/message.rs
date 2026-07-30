use serde::{Deserialize, Serialize};

use crate::{ContentPart, ExtensionMap, MessageId};

/// Conversation role. System instructions are represented separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    /// Human input.
    User,
    /// Agent/provider response.
    Assistant,
    /// Tool-originated content where a provider requires role placement.
    Tool,
    /// A provider role that core preserves but does not interpret.
    ProviderOpaque(String),
}

/// Ordered conversation message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationMessage {
    /// Stable message identity.
    pub id: MessageId,
    /// Provider-neutral role intent.
    pub role: MessageRole,
    /// Ordered content parts.
    pub content: Vec<ContentPart>,
    /// Namespaced metadata.
    #[serde(default)]
    pub extensions: ExtensionMap,
}

/// System instruction kept separate because providers place/cache it differently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemInstruction {
    /// Ordered instruction content.
    pub content: Vec<ContentPart>,
    /// Whether an adapter may treat the instruction as cache-stable.
    pub cache_stable: bool,
    /// Namespaced metadata.
    #[serde(default)]
    pub extensions: ExtensionMap,
}
