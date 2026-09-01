//! V3 prompt ports. The system prompt is a verbatim port of
//! `oracle/configs/prompts.py:ADDITIVE_EXTRACTION_PROMPT` (lines 468-945);
//! `AGENT_CONTEXT_SUFFIX` of `prompts.py:947`; the user-side prompt builder
//! is a port of `prompts.py:1016 generate_additive_extraction_prompt`.
//!
//! Past-message truncation limit mirrors `PAST_MESSAGE_TRUNCATION_LIMIT = 300`.

use serde::{Deserialize, Serialize};

/// Maximum characters of any past message included in the user prompt
/// (oracle: `PAST_MESSAGE_TRUNCATION_LIMIT = 300`).
pub const PAST_MESSAGE_TRUNCATION_LIMIT: usize = 300;

/// Rolling-window size passed to `get_last_messages`.
pub const LAST_K_MESSAGES: usize = 10;

/// Top-k of existing memories fed to the extractor for dedup/linking.
pub const EXISTING_MEMORY_TOP_K: usize = 10;

/// V3 additive extraction system prompt. Ported verbatim from
/// `oracle/configs/prompts.py:468 ADDITIVE_EXTRACTION_PROMPT`.
pub const ADDITIVE_EXTRACTION_PROMPT: &str =
    include_str!("../assets/additive_extraction_prompt.txt");

/// Agent-scoped suffix appended to the system prompt when the scope contains
/// an agent_id and no user_id. Ported verbatim from `oracle/configs/prompts.py:947`.
pub const AGENT_CONTEXT_SUFFIX: &str = include_str!("../assets/agent_context_suffix.txt");

/// Procedural-memory system prompt. Ported verbatim from
/// `oracle/configs/prompts.py:326 PROCEDURAL_MEMORY_SYSTEM_PROMPT`.
pub const PROCEDURAL_MEMORY_SYSTEM_PROMPT: &str =
    include_str!("../assets/procedural_memory_system_prompt.txt");

/// One existing memory row in the format the extractor expects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistingMemory {
    pub id: String,
    pub text: String,
}

/// Truncate to a character limit, appending "..." if shortened. Mirrors the
/// oracle's `_truncate_content`.
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(limit).collect();
        format!("{truncated}...")
    }
}

/// Format message dicts as `role: content\n` lines with truncation.
/// Mirrors `_format_conversation_history`.
fn format_conversation_history(messages: &[crate::types::Message]) -> String {
    let mut out = String::new();
    for msg in messages {
        if msg.role.is_empty() || msg.content.is_empty() {
            continue;
        }
        out.push_str(&msg.role);
        out.push_str(": ");
        out.push_str(&truncate(&msg.content, PAST_MESSAGE_TRUNCATION_LIMIT));
        out.push('\n');
    }
    out
}

/// Serialize the conversation as a JSON array, mirroring `_format_new_messages`.
fn format_new_messages(messages: &[crate::types::Message]) -> String {
    let value: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
            })
        })
        .collect();
    serde_json::to_string(&value).unwrap_or_else(|_| "[]".to_string())
}

/// Build the user-side extraction prompt. Verbatim port of
/// `oracle/configs/prompts.py:1016 generate_additive_extraction_prompt`.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn generate_additive_extraction_prompt(
    summary: Option<&str>,
    recently_extracted_memories: &[String],
    existing_memories: &[ExistingMemory],
    new_messages: &[crate::types::Message],
    last_k_messages: &[crate::types::Message],
    observation_date: &str,
    current_date: &str,
    custom_instructions: Option<&str>,
) -> String {
    let mut sections: Vec<String> = Vec::new();

    sections.push(format!("## Summary\n{}", summary.unwrap_or("").trim()));
    sections.push(format!(
        "## Last k Messages\n{}",
        format_conversation_history(last_k_messages)
    ));
    let recent_json =
        serde_json::to_string(recently_extracted_memories).unwrap_or_else(|_| "[]".to_string());
    sections.push(format!("## Recently Extracted Memories\n{recent_json}"));
    let existing_json =
        serde_json::to_string(existing_memories).unwrap_or_else(|_| "[]".to_string());
    sections.push(format!("## Existing Memories\n{existing_json}"));
    sections.push(format!(
        "## New Messages\n{}",
        format_new_messages(new_messages)
    ));
    sections.push(format!("## Observation Date\n{observation_date}"));
    sections.push(format!("## Current Date\n{current_date}"));
    if let Some(instructions) = custom_instructions {
        sections.push(format!("## Custom Instructions\n{instructions}"));
    }
    sections.push(String::from("# Output:"));
    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_builder_emits_all_sections() {
        let msg = crate::types::Message::user("Hi, I'm Marcus");
        let existing = vec![ExistingMemory {
            id: "0".to_string(),
            text: "User's name is Marcus".to_string(),
        }];
        let prompt = generate_additive_extraction_prompt(
            Some("Profile summary"),
            &["User's name is Marcus".to_string()],
            &existing,
            std::slice::from_ref(&msg),
            &[],
            "2025-08-19",
            "2026-02-18",
            None,
        );
        assert!(prompt.contains("## Summary"));
        assert!(prompt.contains("## Last k Messages"));
        assert!(prompt.contains("## Recently Extracted Memories"));
        assert!(prompt.contains("## Existing Memories"));
        assert!(prompt.contains("## New Messages"));
        assert!(prompt.contains("## Observation Date\n2025-08-19"));
        assert!(prompt.contains("## Current Date\n2026-02-18"));
        assert!(prompt.ends_with("# Output:"));
    }

    #[test]
    fn truncation_appends_ellipsis_only_when_shortened() {
        let short = "hello";
        assert_eq!(truncate(short, 100), short);
        let long = "a".repeat(500);
        let t = truncate(&long, 10);
        assert!(t.ends_with("..."));
        assert_eq!(t.chars().count(), 13);
    }
}
