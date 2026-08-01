#![forbid(unsafe_code)]
//! Secret-safe trajectory recording for the composed agent harness.
//!
//! Recording is opt-in, bounded, and deliberately excludes prompts, message
//! bodies, tool arguments/results, paths, commands, reasoning, and anything
//! whose key looks credential-shaped. The aggregate view is useful for the
//! old harness's observability surface without turning telemetry into a
//! second transcript store.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Maximum serialized event size.
pub const MAX_EVENT_BYTES: usize = 16 * 1024;
/// Maximum events retained by one snapshot query.
pub const MAX_SNAPSHOT_EVENTS: usize = 10_000;
/// Maximum fields in one event.
pub const MAX_FIELDS: usize = 24;

/// Secret-safe trajectory row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryEvent {
    /// Event name, not a free-form message.
    pub event: String,
    /// Opaque session id.
    pub session_id: String,
    /// Unix milliseconds.
    pub at_ms: u128,
    /// Whitelisted scalar metrics only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

/// A bounded, opt-in JSONL recorder.
pub struct TrajectoryRecorder {
    path: Option<PathBuf>,
    enabled: bool,
    lock: Mutex<()>,
}

impl std::fmt::Debug for TrajectoryRecorder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrajectoryRecorder")
            .field("enabled", &self.enabled)
            .field("path_configured", &self.path.is_some())
            .finish_non_exhaustive()
    }
}

impl TrajectoryRecorder {
    /// Creates a recorder. No directory or file is created until `record`.
    #[must_use]
    pub fn new(path: Option<PathBuf>, enabled: bool) -> Self {
        Self {
            path,
            enabled,
            lock: Mutex::new(()),
        }
    }

    /// Disabled recorder used by hosts that do not opt into telemetry.
    #[must_use]
    pub fn disabled() -> Self {
        Self::new(None, false)
    }

    /// Records one allowlisted event. Invalid/sensitive fields are ignored;
    /// an event with no safe fields is still useful for counting.
    pub fn record<I, K, V>(
        &self,
        event: &str,
        session_id: &str,
        fields: I,
    ) -> Result<(), ObservabilityError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        if !self.enabled {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        if event.is_empty() || event.len() > 128 || session_id.len() > 256 {
            return Err(ObservabilityError::Bounds);
        }
        let mut safe_fields = BTreeMap::new();
        for (key, value) in fields.into_iter().take(MAX_FIELDS) {
            let key = key.into();
            let value = value.into();
            if safe_key(&key) && value.len() <= 512 {
                safe_fields.insert(key, value);
            }
        }
        let row = TrajectoryEvent {
            event: event.to_owned(),
            session_id: session_id.to_owned(),
            at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or_default(),
            fields: safe_fields,
        };
        let bytes = serde_json::to_vec(&row).map_err(|_| ObservabilityError::Serialize)?;
        if bytes.len() > MAX_EVENT_BYTES {
            return Err(ObservabilityError::Bounds);
        }
        let _guard = self.lock.lock().map_err(|_| ObservabilityError::Lock)?;
        let parent = path.parent().ok_or(ObservabilityError::InvalidPath)?;
        std::fs::create_dir_all(parent).map_err(|_| ObservabilityError::Io)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|_| ObservabilityError::Io)?;
        file.write_all(&bytes).map_err(|_| ObservabilityError::Io)?;
        file.write_all(b"\n").map_err(|_| ObservabilityError::Io)?;
        file.sync_data().map_err(|_| ObservabilityError::Io)?;
        Ok(())
    }

    /// Reads and aggregates the bounded event stream.
    pub fn snapshot(&self) -> Result<ObservabilitySnapshot, ObservabilityError> {
        let Some(path) = &self.path else {
            return Ok(ObservabilitySnapshot::default());
        };
        snapshot_path(path)
    }
}

/// Aggregated reliability view.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservabilitySnapshot {
    /// Count by event name.
    pub event_counts: BTreeMap<String, u64>,
    /// Count by session id (opaque identifiers only).
    pub session_counts: BTreeMap<String, u64>,
    /// Observed latency values in milliseconds, when supplied by callers.
    pub latency_ms: Vec<u64>,
    /// Number of malformed or over-bound rows skipped.
    pub skipped_rows: u64,
}

impl ObservabilitySnapshot {
    /// Returns the integer p95 latency, or `None` when no latency was logged.
    #[must_use]
    pub fn p95_latency_ms(&self) -> Option<u64> {
        percentile(&self.latency_ms, 95)
    }
}

/// Secret-safe errors suitable for a status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ObservabilityError {
    #[error("observability input exceeded a safety bound")]
    Bounds,
    #[error("observability record could not be serialized")]
    Serialize,
    #[error("observability lock unavailable")]
    Lock,
    #[error("observability path is invalid")]
    InvalidPath,
    #[error("observability storage failed")]
    Io,
}

fn safe_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    ![
        "prompt",
        "message",
        "content",
        "output",
        "argument",
        "reasoning",
        "path",
        "command",
        "secret",
        "token",
        "password",
        "credential",
        "api_key",
        "private_key",
    ]
    .iter()
    .any(|blocked| normalized.contains(blocked))
}

fn snapshot_path(path: &Path) -> Result<ObservabilitySnapshot, ObservabilityError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(_) => return Err(ObservabilityError::Io),
    };
    let mut snapshot = ObservabilitySnapshot::default();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .rev()
        .take(MAX_SNAPSHOT_EVENTS)
    {
        let Ok(event) = serde_json::from_slice::<TrajectoryEvent>(line) else {
            if !line.is_empty() {
                snapshot.skipped_rows = snapshot.skipped_rows.saturating_add(1);
            }
            continue;
        };
        *snapshot.event_counts.entry(event.event).or_default() += 1;
        *snapshot.session_counts.entry(event.session_id).or_default() += 1;
        if let Some(value) = event.fields.get("latency_ms")
            && let Ok(value) = value.parse::<u64>()
        {
            snapshot.latency_ms.push(value);
        }
    }
    snapshot.latency_ms.sort_unstable();
    Ok(snapshot)
}

fn percentile(values: &[u64], percentage: usize) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let index = ((values.len() - 1) * percentage / 100).min(values.len() - 1);
    values.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn recorder_excludes_sensitive_fields_and_aggregates_latency() {
        let temp = TempDir::new().unwrap();
        let recorder = TrajectoryRecorder::new(Some(temp.path().join("trajectory.jsonl")), true);
        recorder
            .record(
                "tool.completed",
                "sess-1",
                [
                    ("latency_ms", "20"),
                    ("prompt", "must not persist"),
                    ("status", "ok"),
                ],
            )
            .unwrap();
        let snapshot = recorder.snapshot().unwrap();
        assert_eq!(snapshot.event_counts["tool.completed"], 1);
        assert_eq!(snapshot.p95_latency_ms(), Some(20));
        let body = std::fs::read_to_string(temp.path().join("trajectory.jsonl")).unwrap();
        assert!(!body.contains("must not persist"));
    }

    #[test]
    fn disabled_recorder_does_not_create_storage() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("missing.jsonl");
        TrajectoryRecorder::new(Some(path.clone()), false)
            .record("turn", "sess", std::iter::empty::<(String, String)>())
            .unwrap();
        assert!(!path.exists());
    }
}
