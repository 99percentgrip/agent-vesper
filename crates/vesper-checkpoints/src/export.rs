//! Session exporter — `/export` and `/export last`.
//!
//! Writes a bounded markdown file capturing the transcript + lineage. The
//! TUI supplies the transcript lines and the lineage chain; the exporter
//! formats them and writes atomically to `<root>/exports/<timestamp>.md`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::CheckpointError;
use crate::io::write_atomic;
use crate::types::SessionRecord;

/// Maximum bytes of an exported markdown file.
pub const MAX_EXPORT_BYTES: usize = 1024 * 1024;

/// Builds and writes one session export.
pub struct SessionExporter {
    root: PathBuf,
}

impl SessionExporter {
    /// Opens an exporter rooted at `root`. The root must be absolute with
    /// an existing parent.
    pub fn open(root: &Path) -> Result<Self, CheckpointError> {
        if !root.is_absolute() {
            return Err(CheckpointError::InvalidRoot);
        }
        match root.parent() {
            Some(parent) if parent.exists() => {}
            Some(_) | None => return Err(CheckpointError::InvalidRoot),
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Writes the export and returns the absolute path of the written file.
    pub fn export(
        &self,
        transcript: &[String],
        lineage: &[SessionRecord],
    ) -> Result<PathBuf, CheckpointError> {
        let body = render_markdown(transcript, lineage);
        self.write_export("session", &body)
    }

    /// Writes only the most recent assistant response in the transcript.
    ///
    /// This mirrors the oracle's `/export last` behavior: the response body is
    /// exported without the transcript role prefix or session lineage.
    pub fn export_last_response(&self, transcript: &[String]) -> Result<PathBuf, CheckpointError> {
        let mut response_lines = Vec::new();
        for line in transcript.iter().rev() {
            let Some(response) = line.strip_prefix("assistant:") else {
                if response_lines.is_empty() {
                    continue;
                }
                break;
            };
            let response = response.trim_start();
            if !response.trim().is_empty() {
                response_lines.push(response);
            }
        }
        if response_lines.is_empty() {
            return Err(CheckpointError::Unavailable("no response to export"));
        }
        response_lines.reverse();
        let response = response_lines.join("\n");
        let body = format!("# GLM ACP Response\n\n{response}\n");
        self.write_export("response", &body)
    }

    fn write_export(&self, prefix: &str, body: &str) -> Result<PathBuf, CheckpointError> {
        if body.len() > MAX_EXPORT_BYTES {
            return Err(CheckpointError::BoundsViolated("export size"));
        }
        let exports_dir = self.root.join("exports");
        std::fs::create_dir_all(&exports_dir).map_err(|_| CheckpointError::io("create"))?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let target = exports_dir.join(format!("{prefix}-{timestamp}.md"));
        write_atomic(&target, body.as_bytes())?;
        Ok(target)
    }
}

/// Renders the export body. Pure: no I/O.
fn render_markdown(transcript: &[String], lineage: &[SessionRecord]) -> String {
    let mut buffer = String::new();
    buffer.push_str("# Agent Vesper session export\n\n");
    buffer.push_str("## Lineage\n\n");
    if lineage.is_empty() {
        buffer.push_str("_(no session lineage recorded)_\n\n");
    } else {
        for record in lineage {
            buffer.push_str(&format!(
                "- **{}** (`{}`) — {:?}\n",
                record.name, record.id, record.status
            ));
        }
        buffer.push('\n');
    }
    buffer.push_str("## Transcript\n\n");
    if transcript.is_empty() {
        buffer.push_str("_(empty transcript)_\n");
    } else {
        for line in transcript {
            buffer.push_str(line);
            buffer.push('\n');
        }
    }
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionStatus;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn export_writes_a_markdown_file_with_transcript_and_lineage() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let exporter = SessionExporter::open(&root).unwrap();
        let transcript = vec!["user: hello".to_string(), "assistant: hi".to_string()];
        let lineage = vec![SessionRecord {
            id: "sess-1".into(),
            parent_id: None,
            name: "alpha".into(),
            workspace_root: "/tmp/ws".into(),
            status: SessionStatus::Active,
            created_at: UNIX_EPOCH,
            updated_at: UNIX_EPOCH,
        }];
        let path = exporter.export(&transcript, &lineage).unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("# Agent Vesper session export"));
        assert!(body.contains("alpha"));
        assert!(body.contains("user: hello"));
    }

    #[test]
    fn export_rejects_oversized_body() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let exporter = SessionExporter::open(&root).unwrap();
        let huge_transcript: Vec<String> = (0..100_000).map(|_| "x".repeat(20)).collect();
        let err = exporter.export(&huge_transcript, &[]).unwrap_err();
        assert_eq!(err, CheckpointError::BoundsViolated("export size"));
    }

    #[test]
    fn export_last_response_writes_only_the_final_assistant_payload() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let exporter = SessionExporter::open(&root).unwrap();
        let transcript = vec![
            "assistant: older response".to_string(),
            "agent: 1 turn(s), 0 tool result(s)".to_string(),
            "user: follow-up".to_string(),
            "assistant: final response, part one".to_string(),
            "assistant: final response, part two".to_string(),
            "agent: 1 turn(s), 0 tool result(s)".to_string(),
        ];

        let path = exporter.export_last_response(&transcript).unwrap();
        let body = fs::read_to_string(&path).unwrap();

        assert_eq!(
            body,
            "# GLM ACP Response\n\nfinal response, part one\nfinal response, part two\n"
        );
        assert!(
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("response-") && name.ends_with(".md"))
        );
    }

    #[test]
    fn export_last_response_requires_an_assistant_payload() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("checkpoint-root");
        fs::create_dir_all(&root).unwrap();
        let exporter = SessionExporter::open(&root).unwrap();

        let err = exporter
            .export_last_response(&["user: hello".to_string()])
            .unwrap_err();
        assert_eq!(err, CheckpointError::Unavailable("no response to export"));
    }
}
