use std::{path::PathBuf, time::SystemTime};

use serde_json::{Map, Value};
use vesper_domain::SessionId;

use crate::{MetadataOrigin, SessionMetadata, SessionSource};

pub(crate) const MAX_METADATA_FIELDS: usize = 64;
pub(crate) const MAX_FALLBACK_FIELDS: usize = 256;
pub(crate) const MAX_METADATA_NODES: usize = 100_000;
pub(crate) const MAX_METADATA_DEPTH: usize = 64;
pub(crate) const MAX_TITLE_BYTES: usize = 1_024;
pub(crate) const MAX_CWD_BYTES: usize = 4_096;
pub(crate) const MAX_TIMESTAMP_BYTES: usize = 128;
pub(crate) const MAX_MODEL_BYTES: usize = 256;
pub(crate) const MAX_PROVIDER_BYTES: usize = 64;
pub(crate) const MAX_LINEAGE_BYTES: usize = 256;

pub(crate) struct MetadataContext {
    pub session_id: SessionId,
    pub source: SessionSource,
    pub byte_len: u64,
    pub modified: Option<SystemTime>,
    pub record_path: Option<PathBuf>,
    pub metadata_path: Option<PathBuf>,
}

pub(crate) fn decode_sidecar(bytes: &[u8], context: MetadataContext) -> Option<SessionMetadata> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    bounded_structure(&value)?;
    let fields = value.as_object()?;
    if fields.len() > MAX_METADATA_FIELDS {
        return None;
    }
    let sidecar_id = required_string(fields, "session_id", 256)?;
    if sidecar_id != context.session_id.as_str() {
        return None;
    }
    metadata_from_fields(fields, context, MetadataOrigin::Sidecar)
}

pub(crate) fn decode_json_fallback(
    bytes: &[u8],
    context: MetadataContext,
) -> Option<SessionMetadata> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    bounded_structure(&value)?;
    let fields = value.as_object()?;
    if fields.len() > MAX_FALLBACK_FIELDS {
        return None;
    }
    metadata_from_fields(fields, context, MetadataOrigin::JsonFallback)
}

fn bounded_structure(value: &Value) -> Option<()> {
    let mut stack = vec![(value, 0_usize)];
    let mut nodes = 0_usize;
    while let Some((value, depth)) = stack.pop() {
        nodes = nodes.checked_add(1)?;
        if nodes > MAX_METADATA_NODES || depth > MAX_METADATA_DEPTH {
            return None;
        }
        match value {
            Value::Array(values) => {
                stack.extend(values.iter().map(|child| (child, depth + 1)));
            }
            Value::Object(values) => {
                stack.extend(values.values().map(|child| (child, depth + 1)));
            }
            _ => {}
        }
    }
    Some(())
}

fn metadata_from_fields(
    fields: &Map<String, Value>,
    context: MetadataContext,
    origin: MetadataOrigin,
) -> Option<SessionMetadata> {
    let cwd = optional_string(fields, "cwd", MAX_CWD_BYTES)
        .ok()?
        .or_else(|| vesper_primary_root(fields))
        .unwrap_or_default();
    let title = optional_string(fields, "title", MAX_TITLE_BYTES).ok()?;
    let updated_at = optional_string(fields, "updated_at", MAX_TIMESTAMP_BYTES)
        .ok()?
        .or(optional_string(fields, "saved_at", MAX_TIMESTAMP_BYTES).ok()?);
    let parent_session_id = optional_string(fields, "parent_session_id", MAX_LINEAGE_BYTES)
        .ok()?
        .or_else(|| nested_string(fields, "lineage", "parent_session_id", MAX_LINEAGE_BYTES));
    let branch_root_id = optional_string(fields, "branch_root_id", MAX_LINEAGE_BYTES)
        .ok()?
        .or_else(|| nested_string(fields, "lineage", "root_session_id", MAX_LINEAGE_BYTES))
        .or_else(|| Some(context.session_id.as_str().to_owned()));
    let model = optional_string(fields, "model", MAX_MODEL_BYTES)
        .ok()
        .flatten()
        .or_else(|| nested_string(fields, "model", "model_id", MAX_MODEL_BYTES));
    let provider = optional_string(fields, "provider", MAX_PROVIDER_BYTES)
        .ok()
        .flatten()
        .or_else(|| {
            optional_string(fields, "provider_id", MAX_PROVIDER_BYTES)
                .ok()
                .flatten()
        });
    Some(SessionMetadata {
        session_id: context.session_id,
        source: context.source.clone(),
        byte_len: context.byte_len,
        modified: context.modified,
        record_path: context.record_path,
        metadata_path: context.metadata_path,
        origin,
        title,
        cwd,
        updated_at,
        model,
        provider,
        parent_session_id,
        branch_root_id,
        // Legacy records have no independently redacted preview contract.
        safe_preview: None,
        read_only: context.source != SessionSource::InMemory,
    })
}

fn vesper_primary_root(fields: &Map<String, Value>) -> Option<String> {
    fields
        .get("workspace_roots")?
        .as_array()?
        .iter()
        .find_map(|value| {
            let root = value.as_object()?;
            if root.get("primary")?.as_bool()? {
                optional_string(root, "path", MAX_CWD_BYTES).ok().flatten()
            } else {
                None
            }
        })
}

fn nested_string(
    fields: &Map<String, Value>,
    object: &str,
    field: &str,
    maximum: usize,
) -> Option<String> {
    optional_string(fields.get(object)?.as_object()?, field, maximum)
        .ok()
        .flatten()
}

fn required_string(fields: &Map<String, Value>, field: &str, maximum: usize) -> Option<String> {
    optional_string(fields, field, maximum).ok().flatten()
}

fn optional_string(
    fields: &Map<String, Value>,
    field: &str,
    maximum: usize,
) -> Result<Option<String>, ()> {
    match fields.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.len() <= maximum => Ok(Some(value.clone())),
        Some(_) => Err(()),
    }
}

/// Newest timestamp first; session ID is the deterministic ascending tie-breaker.
pub fn sort_session_metadata(values: &mut [SessionMetadata]) {
    values.sort_by(|left, right| {
        right
            .updated_at
            .as_deref()
            .unwrap_or("")
            .cmp(left.updated_at.as_deref().unwrap_or(""))
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
}
