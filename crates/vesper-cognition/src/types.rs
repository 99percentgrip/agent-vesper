//! Public value types for the cognitive-memory engine. Mirrors the mem0 V3
//! data model where applicable; documented inline.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Who a memory or message is attributed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Attribution {
    User,
    Assistant,
}

impl Attribution {
    /// Stable string tag for storage and SQL filters.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Attribution::User => "user",
            Attribution::Assistant => "assistant",
        }
    }

    /// Parse a stored tag back into the enum. Unknown strings map to `None`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            _ => None,
        }
    }
}

/// Session scope: at least one of these identifiers is required for every
/// operation (mirrors mem0's hard requirement).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub user_id: Option<String>,
    pub agent_id: Option<String>,
    pub run_id: Option<String>,
}

impl Scope {
    /// Returns `true` if at least one identifier is set.
    #[must_use]
    pub fn is_set(&self) -> bool {
        self.user_id.is_some() || self.agent_id.is_some() || self.run_id.is_some()
    }

    /// Deterministic session-scope key matching mem0's `_build_session_scope`
    /// (sorted `key=value` joined with `&`). Used as the SQLite messages-table
    /// partition key.
    #[must_use]
    pub fn session_key(&self) -> String {
        let mut parts = Vec::new();
        if let Some(user) = &self.user_id {
            parts.push(format!("user_id={user}"));
        }
        if let Some(agent) = &self.agent_id {
            parts.push(format!("agent_id={agent}"));
        }
        if let Some(run) = &self.run_id {
            parts.push(format!("run_id={run}"));
        }
        parts.join("&")
    }
}

/// Conversation message — minimal OpenAI-style role/content shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// Optional named speaker (mem0 multi-speaker chats).
    pub name: Option<String>,
}

impl Message {
    /// Construct a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: String::from("user"),
            content: content.into(),
            name: None,
        }
    }

    /// Construct an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: String::from("assistant"),
            content: content.into(),
            name: None,
        }
    }
}

/// Stored memory record (mirrors mem0's vector-store payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct MemoryRecord {
    pub id: String,
    pub data: String,
    pub hash: String,
    pub text_lemmatized: String,
    pub attributed_to: Option<Attribution>,
    pub scope: Scope,
    pub actor_id: Option<String>,
    pub role: Option<String>,
    pub memory_type: Option<String>,
    pub expiration_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub extras: BTreeMap<String, serde_json::Value>,
}

/// Search hit — the public return type of `CognitiveMemory::search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub id: String,
    pub memory: String,
    pub score: f32,
    pub hash: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub attributed_to: Option<Attribution>,
    pub scope: Scope,
    pub extras: BTreeMap<String, serde_json::Value>,
    /// Only populated when `explain=true`.
    pub score_details: Option<ScoreBreakdown>,
}

/// Score decomposition for `explain=true` queries. Mirrors mem0's
/// `score_details` shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub semantic_score: f32,
    pub bm25_score: f32,
    pub entity_boost: f32,
    pub raw_score: f32,
    pub max_possible_score: f32,
    pub final_score: f32,
    pub threshold: f32,
}

/// Memory-add event returned from `add()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub id: String,
    pub memory: String,
    pub event: &'static str,
}

/// History audit-log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub id: String,
    pub memory_id: String,
    pub old_memory: Option<String>,
    pub new_memory: Option<String>,
    pub event: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub is_deleted: bool,
    pub actor_id: Option<String>,
    pub role: Option<String>,
}
