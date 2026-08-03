//! Extraction-response parsing. Mirrors mem0's defensive chain:
//! `remove_code_blocks` -> `json.loads` -> `extract_json` regex fallback
//! (`mem0/memory/utils.py:115, 131`).
//!
//! The LLM is asked to emit JSON with `response_format=json_object`, but
//! not every provider honors it, so we strip code-block fences and fall
//! back to a brace-matching JSON extractor on parse failure.

use serde::{Deserialize, Serialize};

use crate::error::{CognitionError, Result};

/// One extracted memory as emitted by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedMemory {
    /// Sequential integer string assigned by the LLM ("0", "1", ...).
    /// Anti-hallucination: never used as a real ID.
    #[serde(default)]
    pub id: String,
    pub text: String,
    /// "user" or "assistant" — required by the prompt schema.
    #[serde(default)]
    pub attributed_to: Option<String>,
    /// Optional references to Existing Memory UUIDs for entity linking.
    /// Per ADR 0015 these are NOT persisted in OSS V3 (matches mem0).
    #[serde(default)]
    pub linked_memory_ids: Vec<String>,
}

#[derive(Deserialize)]
struct ExtractionResponse {
    #[serde(default)]
    memory: Vec<ExtractedMemory>,
}

/// Strip Markdown code-block fences (` ```json ... ``` ` or ` ``` ... ``` `)
/// surrounding a JSON payload. Mirrors `remove_code_blocks`.
#[must_use]
pub fn remove_code_blocks(content: &str) -> String {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return content.to_string();
    }
    let after_open = &trimmed[3..];
    // Skip an optional language tag on the opening fence line.
    let body_start = match after_open.find('\n') {
        Some(idx) => idx + 1,
        None => return content.to_string(),
    };
    let body_with_close = &after_open[body_start..];
    if let Some(end) = body_with_close.rfind("```") {
        body_with_close[..end].to_string()
    } else {
        body_with_close.to_string()
    }
}

/// Brace-matching JSON extractor. Mirrors `extract_json`: scan for the first
/// `{`, then balance braces (respecting string literals and escapes) until
/// the matching `}`. Returns the substring or `None` if no balanced object
/// exists.
#[must_use]
pub fn extract_json(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut start = None;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate() {
        let c = b as char;
        if escape {
            escape = false;
            continue;
        }
        if in_str {
            if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0
                    && let Some(s) = start
                {
                    return Some(text[s..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse the LLM response into a list of extracted memories. Applies the
/// defensive chain: strip fences -> parse -> brace-fallback -> [].
///
/// Returns `Ok(vec![])` (NOT an error) when nothing was extracted; the only
/// error path is when the response is non-empty but cannot be parsed at all
/// (which surfaces as `ExtractionParse` so the composition boundary can
/// distinguish "parse failure" from "nothing extracted").
pub fn parse_extraction_response(response: &str) -> Result<Vec<ExtractedMemory>> {
    let stripped = remove_code_blocks(response);
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(parsed) = serde_json::from_str::<ExtractionResponse>(trimmed) {
        return Ok(parsed.memory);
    }
    if let Some(json_substring) = extract_json(trimmed)
        && let Ok(parsed) = serde_json::from_str::<ExtractionResponse>(&json_substring)
    {
        return Ok(parsed.memory);
    }
    Err(CognitionError::ExtractionParse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json_object() {
        let r = r#"{"memory":[{"id":"0","text":"User likes Rust","attributed_to":"user"}]}"#;
        let out = parse_extraction_response(r).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "User likes Rust");
        assert_eq!(out[0].attributed_to.as_deref(), Some("user"));
    }

    #[test]
    fn parses_empty_memory() {
        let r = r#"{"memory":[]}"#;
        let out = parse_extraction_response(r).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn empty_response_is_empty_not_error() {
        let out = parse_extraction_response("").unwrap();
        assert!(out.is_empty());
        let out = parse_extraction_response("   ").unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn strips_code_block_fences() {
        let r =
            "```json\n{\"memory\":[{\"id\":\"0\",\"text\":\"x\",\"attributed_to\":\"user\"}]}\n```";
        let out = parse_extraction_response(r).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "x");
    }

    #[test]
    fn brace_fallback_rescues_prose_wrapped_json() {
        let r = "Here is the response: {\"memory\":[{\"id\":\"0\",\"text\":\"y\",\"attributed_to\":\"assistant\"}]} hope this helps!";
        let out = parse_extraction_response(r).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "y");
    }

    #[test]
    fn unparseable_nonempty_response_errors() {
        let r = "the cat sat on the mat";
        assert!(parse_extraction_response(r).is_err());
    }

    #[test]
    fn extract_json_balances_braces_in_strings() {
        // The `{` inside the string must NOT count toward brace depth.
        let r = r#"prefix {"a":{"b":"{not a brace}"},"c":1} suffix"#;
        let extracted = extract_json(r).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&extracted).unwrap();
        assert_eq!(parsed["c"], 1);
    }
}
