//! Bounded, read-only persisted session search.
//!
//! The frozen Python harness uses a rebuildable SQLite FTS index. Agent Vesper
//! deliberately keeps foundation storage SQLite-free, so this module provides
//! the same observable search port with a bounded linear scan over the existing
//! session repositories. It never writes an index, never searches reasoning or
//! tool payloads, and fails closed on malformed records.

use serde_json::Value;

use crate::{
    BoxSessionFuture, SessionListFilter, SessionReadIntent, SessionRepository, SessionSearchHit,
    SessionSearchMessage, SessionSearchRequest, SessionStoreError,
};

const MAX_QUERY_BYTES: usize = 1_024;
const MAX_RESULTS: usize = 20;
const MAX_WINDOW: usize = 20;
const MAX_MESSAGES: usize = 10_000;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_SNIPPET_BYTES: usize = 4_000;

/// Searches persisted user/assistant messages through a read-only repository.
pub fn search_sessions<'a>(
    repository: &'a dyn SessionRepository,
    request: SessionSearchRequest,
) -> BoxSessionFuture<'a, Result<Vec<SessionSearchHit>, SessionStoreError>> {
    Box::pin(async move {
        if request.query.len() > MAX_QUERY_BYTES {
            return Err(SessionStoreError::SearchQueryTooLong {
                maximum: MAX_QUERY_BYTES,
            });
        }
        let limit = request.limit.clamp(1, MAX_RESULTS);
        let window = request.window.clamp(1, MAX_WINDOW);
        let query = request.query.trim().to_lowercase();
        let metadata = repository
            .list_filtered(SessionListFilter::default())
            .await?;
        let mut hits = Vec::new();

        for entry in metadata {
            if request
                .session_id
                .as_ref()
                .is_some_and(|id| id != &entry.session_id)
            {
                continue;
            }
            let Some(record) = repository
                .read(&entry.session_id, SessionReadIntent::Replay)
                .await?
            else {
                continue;
            };
            let messages = visible_messages(&record.bytes);
            if messages.is_empty() {
                continue;
            }

            let candidate_ordinals: Vec<usize> = if let Some(anchor) = request.around_ordinal {
                if messages.iter().any(|message| message.ordinal == anchor) {
                    vec![anchor]
                } else {
                    Vec::new()
                }
            } else {
                messages
                    .iter()
                    .filter(|message| {
                        query.is_empty() || message.text.to_lowercase().contains(&query)
                    })
                    .map(|message| message.ordinal)
                    .collect()
            };

            for ordinal in candidate_ordinals {
                let Some(message) = messages.iter().find(|message| message.ordinal == ordinal)
                else {
                    continue;
                };
                if !query.is_empty() && !message.text.to_lowercase().contains(&query) {
                    continue;
                }
                let Some(anchor_position) = messages
                    .iter()
                    .position(|candidate| candidate.ordinal == ordinal)
                else {
                    continue;
                };
                let start = anchor_position.saturating_sub(window);
                let end = anchor_position
                    .saturating_add(window)
                    .saturating_add(1)
                    .min(messages.len());
                hits.push(SessionSearchHit {
                    session_id: entry.session_id.clone(),
                    source: entry.source.clone(),
                    ordinal,
                    role: message.role.clone(),
                    snippet: snippet(&message.text, &query),
                    context: messages[start..end].to_vec(),
                    score: relevance(&message.text, &query),
                });
            }
        }

        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.session_id.cmp(&right.session_id))
                .then_with(|| left.ordinal.cmp(&right.ordinal))
        });
        hits.truncate(limit);
        Ok(hits)
    })
}

fn visible_messages(bytes: &[u8]) -> Vec<SessionSearchMessage> {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    let Some(messages) = value
        .get("messages")
        .or_else(|| value.get("history"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    messages
        .iter()
        .take(MAX_MESSAGES)
        .enumerate()
        .filter_map(|(ordinal, message)| {
            let object = message.as_object()?;
            let role = object.get("role")?.as_str()?;
            if !matches!(role, "user" | "assistant") {
                return None;
            }
            let text = extract_text(object.get("content")?)?;
            if text.is_empty() {
                return None;
            }
            Some(SessionSearchMessage {
                ordinal,
                role: role.to_owned(),
                text,
            })
        })
        .collect()
}

fn extract_text(value: &Value) -> Option<String> {
    let mut pieces = Vec::new();
    collect_text(value, 0, &mut pieces);
    if pieces.is_empty() {
        return None;
    }
    let mut text = pieces.join("\n");
    if text.len() > MAX_TEXT_BYTES {
        text.truncate(MAX_TEXT_BYTES);
    }
    Some(text)
}

fn collect_text(value: &Value, depth: usize, pieces: &mut Vec<String>) {
    if depth > 8 {
        return;
    }
    match value {
        Value::String(text) if !text.is_empty() => pieces.push(text.clone()),
        Value::Array(values) => {
            for value in values.iter().take(128) {
                collect_text(value, depth + 1, pieces);
            }
        }
        Value::Object(fields) => {
            if let Some(text) = fields.get("text") {
                collect_text(text, depth + 1, pieces);
            } else if let Some(content) = fields.get("content") {
                collect_text(content, depth + 1, pieces);
            }
        }
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn relevance(text: &str, query: &str) -> u32 {
    if query.is_empty() {
        return 1;
    }
    let lower = text.to_lowercase();
    let occurrences = lower.matches(query).count().min(100) as u32;
    occurrences.saturating_mul(10) + u32::from(lower == query) * 100
}

fn snippet(text: &str, query: &str) -> String {
    if text.len() <= MAX_SNIPPET_BYTES {
        return text.to_owned();
    }
    let start = query
        .is_empty()
        .then_some(0)
        .or_else(|| text.to_lowercase().find(query))
        .unwrap_or(0)
        .saturating_sub(MAX_SNIPPET_BYTES / 3);
    let end = start.saturating_add(MAX_SNIPPET_BYTES).min(text.len());
    let mut start = start;
    while start < end && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = end;
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[start..end].to_owned()
}
