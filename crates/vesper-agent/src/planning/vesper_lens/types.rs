//! Native data types for VesperLens human-in-the-loop feedback (ADR 0017).
//!
//! The wire contract is intentionally minimal — it is the **only** data
//! that crosses back from the browser to the Vesper process. The agent
//! never receives raw HTML back from the review; it receives a parsed
//! [`LensFeedback`] struct.
//!
//! ## Wire shape
//!
//! ```json
//! {
//!   "action": "approve",
//!   "annotations": [
//!     { "selector": "#hero", "comment": "too big", "suggested_html": null }
//!   ],
//!   "notes": "",
//!   "answers": [{ "question": "framework", "value": "Rust" }]
//! }
//! ```

use serde::{Deserialize, Serialize};

/// Whether the human approved, rejected, or wants to modify the artifact.
///
/// Wire form is lowercase (`"approve" | "reject" | "modify"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Artifact is acceptable as-is; resume execution.
    Approve,
    /// Artifact is wrong; resume execution with a reject signal.
    Reject,
    /// Artifact is close but needs changes; resume with annotations.
    Modify,
}

/// A specific DOM node the human highlighted or commented on.
///
/// The `selector` is whatever the overlay could derive (CSS selector, DOM
/// path, or element id) — it is best-effort and must not be assumed to be
/// a unique identifier the agent can blindly re-resolve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomAnnotation {
    /// Stable browser-generated identifier used to update or remove the
    /// corresponding highlight without relying on a fragile selector.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// CSS selector, DOM path, or element id identifying the annotated node.
    pub selector: String,
    /// Free-form human comment about this node. Treated as untrusted user
    /// input by every downstream consumer.
    pub comment: String,
    /// Optional replacement HTML the human supplied for this node.
    /// `None` means "no concrete replacement proposed, just a comment".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_html: Option<String>,
    /// Rich target metadata. Element annotations retain a selector while text
    /// selections additionally preserve exact DOM range boundaries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<AnnotationTarget>,
}

/// Precise browser target attached to an annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AnnotationTarget {
    Element {
        selector: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        tag: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
    },
    TextRange {
        text: String,
        selector: String,
        start: RangeBoundary,
        end: RangeBoundary,
    },
}

/// One anchored endpoint of a selected DOM text range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeBoundary {
    pub selector: String,
    #[serde(default)]
    pub path: Vec<usize>,
    pub offset: usize,
}

/// One structured answer collected from an interactive VesperLens planning
/// question. `question` is the stable id supplied by the tool caller; `value`
/// is the human-selected or typed value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LensAnswer {
    pub question: String,
    pub value: String,
}

/// One bounded planning question rendered by the native VesperLens interview
/// surface. Empty `options` produces a free-text field; otherwise the browser
/// renders radio buttons or checkboxes according to `allow_multiple`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LensQuestion {
    pub id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub allow_multiple: bool,
    #[serde(default = "default_required")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recommended: String,
    #[serde(default)]
    pub allow_other: bool,
}

const fn default_required() -> bool {
    true
}

/// The overarching struct returned by [`crate::planning::vesper_lens::VesperLens::review_artifact`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LensFeedback {
    /// The human's overall verdict.
    pub action: Action,
    /// Per-node annotations (may be empty even on `Modify`).
    #[serde(default)]
    pub annotations: Vec<DomAnnotation>,
    /// Free-form overall notes. Untrusted user input.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Structured planning/interview answers gathered from controls carrying
    /// a `data-vesper-question` id. Empty for ordinary artifact reviews.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<LensAnswer>,
    /// True when the reviewer explicitly ended the reusable browser session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub end_session: bool,
}

impl Default for LensFeedback {
    fn default() -> Self {
        // A safe no-op default used by tests and by the empty-POST guard
        // in the server. Real feedback is always parsed from JSON.
        Self {
            action: Action::Reject,
            annotations: Vec::new(),
            notes: String::new(),
            answers: Vec::new(),
            end_session: false,
        }
    }
}

/// Errors raised by VesperLens.
///
/// All variants are deliberately non-leaking: no HTML payload, no remote
/// address, no headers. The planner can log the variant and continue.
#[derive(Debug, thiserror::Error)]
pub enum LensError {
    /// A `tokio::net` or filesystem I/O failure (bind, accept, read, write).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The raw HTTP/1.1 request failed to parse (malformed request line,
    /// missing Content-Length, body shorter than declared, etc.).
    #[error("http parse error: {0}")]
    HttpParse(String),
    /// The POST body was not valid JSON or did not match the
    /// [`LensFeedback`] schema.
    #[error("json decode error: {0}")]
    Json(#[from] serde_json::Error),
    /// The POST arrived but its body was empty.
    #[error("feedback body was empty")]
    EmptyBody,
    /// The review did not complete within the configured timeout. The
    /// listener is shut down before this is returned.
    #[error("timed out waiting for human review")]
    Timeout,
    /// The reusable server session ended before queued feedback was received.
    #[error("connection closed before feedback received")]
    Disconnected,
    /// The requested artifact was outside the workspace, was not HTML, or
    /// exceeded the bounded review size.
    #[error("invalid review artifact: {0}")]
    InvalidArtifact(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Action::Approve).unwrap(),
            "\"approve\""
        );
        assert_eq!(
            serde_json::to_string(&Action::Reject).unwrap(),
            "\"reject\""
        );
        assert_eq!(
            serde_json::to_string(&Action::Modify).unwrap(),
            "\"modify\""
        );
    }

    #[test]
    fn action_deserializes_lowercase() {
        let a: Action = serde_json::from_str("\"approve\"").unwrap();
        assert_eq!(a, Action::Approve);
        let r: Action = serde_json::from_str("\"reject\"").unwrap();
        assert_eq!(r, Action::Reject);
        let m: Action = serde_json::from_str("\"modify\"").unwrap();
        assert_eq!(m, Action::Modify);
    }

    #[test]
    fn action_rejects_unknown_variants() {
        assert!(serde_json::from_str::<Action>("\"delete\"").is_err());
        assert!(serde_json::from_str::<Action>("\"APPROVE\"").is_err());
    }

    #[test]
    fn lens_feedback_round_trip_with_annotations() {
        let fb = LensFeedback {
            action: Action::Modify,
            annotations: vec![DomAnnotation {
                id: "note-1".into(),
                selector: "#hero".into(),
                comment: "too big".into(),
                suggested_html: Some("<h1>smaller</h1>".into()),
                target: None,
            }],
            notes: "overall fine".into(),
            answers: vec![LensAnswer {
                question: "framework".into(),
                value: "Rust".into(),
            }],
            end_session: false,
        };
        let json = serde_json::to_string(&fb).unwrap();
        let back: LensFeedback = serde_json::from_str(&json).unwrap();
        assert_eq!(fb, back);
    }

    #[test]
    fn lens_feedback_minimal_round_trip_omits_optional_fields() {
        let fb = LensFeedback {
            action: Action::Approve,
            annotations: Vec::new(),
            notes: String::new(),
            answers: Vec::new(),
            end_session: false,
        };
        let json = serde_json::to_string(&fb).unwrap();
        // notes is skipped when empty; annotations is `[]` (defaulted on
        // decode), suggested_html omitted entirely on annotations.
        assert!(!json.contains("notes"));
        let back: LensFeedback = serde_json::from_str(&json).unwrap();
        assert_eq!(back, fb);
    }

    #[test]
    fn lens_feedback_decodes_minimal_wire() {
        // Approve with no annotations/notes — the absolute minimum the
        // trusted review chrome would ever POST.
        let json = r#"{"action":"approve"}"#;
        let fb: LensFeedback = serde_json::from_str(json).unwrap();
        assert_eq!(fb.action, Action::Approve);
        assert!(fb.annotations.is_empty());
        assert!(fb.notes.is_empty());
        assert!(fb.answers.is_empty());
    }

    #[test]
    fn default_is_safe_noop() {
        let d = LensFeedback::default();
        assert_eq!(d.action, Action::Reject);
        assert!(d.annotations.is_empty());
        assert!(d.notes.is_empty());
        assert!(d.answers.is_empty());
    }

    #[test]
    fn dom_annotation_skips_null_suggested_html_on_serialize() {
        let a = DomAnnotation {
            id: String::new(),
            selector: "p".into(),
            comment: "x".into(),
            suggested_html: None,
            target: None,
        };
        let json = serde_json::to_string(&a).unwrap();
        assert!(!json.contains("suggested_html"));
    }
}
