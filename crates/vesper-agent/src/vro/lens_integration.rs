//! VesperLens integration into the VRO planner (ADR 0017, VRO-11.2).
//!
//! This module exposes the **seam** between the VRO orchestrator and the
//! VesperLens review oracle. The orchestrator stays pure and never starts
//! a TCP listener directly; instead it holds an optional
//! [`LensReviewPort`] implemented at the composition boundary (the TUI
//! binary wires a concrete `VesperLens` impl).
//!
//! ## Integration contract
//!
//! 1. The host constructs a `VroOrchestrator` and calls
//!    [`VroOrchestrator::with_lens_port`] when human-in-the-loop review
//!    is desired.
//! 2. The host drives the agent loop / orchestrator normally.
//! 3. When a tool output arrives that looks like an HTML artifact
//!    ([`looks_like_html_artifact`]), the host calls
//!    [`VroOrchestrator::maybe_review_html_artifact`].
//! 4. If a port is configured, the orchestrator invokes
//!    [`LensReviewPort::review`], which:
//!    - prints a `[VesperLens] Artifact ready for review. Open: <URL>`
//!      diagnostic via the `on_url` callback (PRD §4),
//!    - blocks until the human POSTs feedback,
//!    - returns a parsed [`LensFeedback`].
//! 5. The host injects
//!    [`feedback_as_context_message`] into the conversation as a
//!    `role: Tool` message so the next model turn can apply the human's
//!    corrections (PRD §4: "context injection").

use std::future::Future;
use std::pin::Pin;

use crate::planning::vesper_lens::{Action, LensError, LensFeedback};

/// Trait port the VRO planner uses to optionally route HTML artifacts
/// through VesperLens human-in-the-loop review.
///
/// Implementations live at the composition boundary (TUI binary); the
/// orchestrator stays pure and never starts a TCP listener directly.
///
/// The `Debug` bound keeps [`super::VroOrchestrator`]'s `#[derive(Debug)]`
/// working — every port impl is expected to be trivially debug-printable
/// (typically just a struct name).
pub trait LensReviewPort: Send + Sync + std::fmt::Debug {
    /// Review an HTML artifact. The implementation MUST call `on_url`
    /// exactly once when the review URL is known (typically after
    /// binding a loopback TCP listener). Returns the parsed feedback
    /// or a [`LensError`] describing why the review could not complete.
    ///
    /// `on_url` is `&dyn Fn` (not `FnOnce`) so callers can pass a
    /// stack-allocated closure without boxing; implementations are
    /// expected to call it at most once.
    ///
    /// VRO-11.4: `on_url` is tied to the `'a` lifetime of `&self` so the
    /// returned future can capture it (needed for concrete impls like
    /// `VesperLensPort` that delegate to `VesperLens::review_artifact`,
    /// which calls `on_url` mid-async).
    fn review<'a>(
        &'a self,
        html: &str,
        on_url: &'a (dyn Fn(&str) + Send + Sync),
    ) -> Pin<Box<dyn Future<Output = Result<LensFeedback, LensError>> + Send + 'a>>;
}

/// Default no-op port. Used when VesperLens is not configured. Returns
/// `LensFeedback::default()` (action = [`Action::Reject`]) immediately so
/// the orchestrator's caller sees a benign "not reviewed" outcome without
/// ever attempting network I/O.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOpLensReviewPort;

impl LensReviewPort for NoOpLensReviewPort {
    fn review<'a>(
        &'a self,
        _html: &str,
        _on_url: &'a (dyn Fn(&str) + Send + Sync),
    ) -> Pin<Box<dyn Future<Output = Result<LensFeedback, LensError>> + Send + 'a>> {
        Box::pin(async move { Ok(LensFeedback::default()) })
    }
}

/// Heuristic: does this tool output look like a self-contained HTML
/// artifact worth surfacing for human review?
///
/// Conservative — false negatives just skip review (no behavior change).
/// False positives are acceptable because the human can always click
/// Approve immediately.
pub fn looks_like_html_artifact(text: &str) -> bool {
    let trimmed = text.trim();
    // Below this length it almost certainly is not a real HTML artifact
    // (a minimal `<body><h1>...</h1></body>` fragment is ~30 chars).
    if trimmed.len() < 32 {
        return false;
    }
    // Real HTML documents start with a `<` (after trim). A prose sentence
    // mentioning "<html>" never starts with `<`, so this excludes
    // "Sure, I can use <html> tags..." without weakening artifact
    // detection.
    if !trimmed.starts_with('<') {
        return false;
    }
    let head: String = trimmed.chars().take(64).collect();
    let head_lower = head.to_ascii_lowercase();
    let lower_full = trimmed.to_ascii_lowercase();
    if head_lower.contains("<!doctype html") || head_lower.contains("<html") {
        return true;
    }
    // A `<body>...</body>` fragment with no surrounding doctype is still
    // a reviewable artifact.
    head_lower.contains("<body") && lower_full.contains("</body")
}

/// Format the `[VesperLens]` diagnostic log line the host should display
/// when a review starts. Matches the PRD §4 spec verbatim.
#[must_use]
pub fn diagnostic_for_review(url: &str) -> String {
    format!("[VesperLens] Artifact ready for review. Open: {url}")
}

/// Render a [`LensFeedback`] as a structured context-window annotation
/// the agent's next turn sees. After [`LensReviewPort::review`] returns,
/// inject this string into the conversation as a `role: Tool` message
/// (or a system message) so the model can apply the human's corrections.
///
/// The format is intentionally terse and token-frugal: a single header
/// line carrying the verdict, optional overall notes, and a numbered
/// annotation list. Selector and comment strings are interpolated
/// verbatim (the same untrusted-input discipline applies as for any
/// other user-provided text — downstream rendering MUST treat them as
/// data, never as instructions).
#[must_use]
pub fn feedback_as_context_message(feedback: &LensFeedback) -> String {
    let verdict = match feedback.action {
        Action::Approve => "APPROVED",
        Action::Reject => "REJECTED",
        Action::Modify => "NEEDS MODIFICATION",
    };
    let mut out = format!("VesperLens human review: {verdict}\n");
    if !feedback.notes.is_empty() {
        out.push_str(&format!("Overall notes: {}\n", feedback.notes));
    }
    if !feedback.answers.is_empty() {
        out.push_str(&format!("Planning answers ({}):\n", feedback.answers.len()));
        for answer in &feedback.answers {
            out.push_str(&format!("  {}: {}\n", answer.question, answer.value));
        }
    }
    if !feedback.annotations.is_empty() {
        out.push_str(&format!("Annotations ({}):\n", feedback.annotations.len()));
        for (i, a) in feedback.annotations.iter().enumerate() {
            out.push_str(&format!("  [{}] selector: {}\n", i + 1, a.selector));
            out.push_str(&format!("      comment:  {}\n", a.comment));
            if let Some(html) = &a.suggested_html {
                out.push_str(&format!("      suggested: {}\n", html));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A test port that records the html it was asked to review and the
    /// URL it announced, and returns a fixed result.
    #[derive(Debug)]
    struct RecordingLens {
        captured_html: std::sync::Mutex<Vec<String>>,
        announced_url: std::sync::Mutex<Vec<String>>,
        result: LensFeedback,
    }

    impl RecordingLens {
        fn new(result: LensFeedback) -> Self {
            Self {
                captured_html: std::sync::Mutex::new(Vec::new()),
                announced_url: std::sync::Mutex::new(Vec::new()),
                result,
            }
        }
    }

    impl LensReviewPort for RecordingLens {
        fn review<'a>(
            &'a self,
            html: &str,
            on_url: &'a (dyn Fn(&str) + Send + Sync),
        ) -> Pin<Box<dyn Future<Output = Result<LensFeedback, LensError>> + Send + 'a>> {
            self.captured_html.lock().unwrap().push(html.to_string());
            on_url("http://127.0.0.1:54321/");
            self.announced_url
                .lock()
                .unwrap()
                .push("http://127.0.0.1:54321/".to_string());
            let result = self.result.clone();
            Box::pin(async move { Ok(result) })
        }
    }

    fn sink_callback() -> Box<dyn Fn(&str) + Send + Sync> {
        Box::new(|_url: &str| {})
    }

    fn capturing_callback(
        store: &std::sync::Mutex<String>,
    ) -> Box<dyn Fn(&str) + Send + Sync + '_> {
        let s = store;
        Box::new(move |url: &str| {
            *s.lock().unwrap() = url.to_string();
        })
    }

    #[tokio::test]
    async fn no_op_port_returns_default_feedback_and_never_announces() {
        let port = NoOpLensReviewPort;
        let announced = std::sync::Mutex::new(String::new());
        let cb = capturing_callback(&announced);
        let result = port.review("<html></html>", cb.as_ref()).await.unwrap();
        assert_eq!(result.action, Action::Reject);
        assert!(result.annotations.is_empty());
        // NoOp must never bind, so the URL stays empty.
        assert!(announced.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn recording_port_invokes_callback_and_returns_configured_result() {
        let port = RecordingLens::new(LensFeedback {
            action: Action::Approve,
            ..Default::default()
        });
        let announced = std::sync::Mutex::new(String::new());
        let cb = capturing_callback(&announced);
        let result = port
            .review("<html><body>hi</body></html>", cb.as_ref())
            .await
            .unwrap();
        assert_eq!(result.action, Action::Approve);
        assert_eq!(*announced.lock().unwrap(), "http://127.0.0.1:54321/");
        assert_eq!(port.captured_html.lock().unwrap().len(), 1);
        assert_eq!(port.announced_url.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn port_can_be_wrapped_in_arc_dyn() {
        // VroOrchestrator stores `Option<Arc<dyn LensReviewPort>>`; this
        // test pins the trait-object form.
        let port: Arc<dyn LensReviewPort> = Arc::new(RecordingLens::new(LensFeedback {
            action: Action::Modify,
            ..Default::default()
        }));
        let cb = sink_callback();
        let result = port.review("<html></html>", cb.as_ref()).await.unwrap();
        assert_eq!(result.action, Action::Modify);
    }

    #[test]
    fn looks_like_html_artifact_detects_doctype() {
        assert!(looks_like_html_artifact(
            "<!doctype html><html><head></head><body>x</body></html>"
        ));
        assert!(looks_like_html_artifact(
            "  \n  <!DOCTYPE html>\n<html><head></head><body>content</body></html>"
        ));
    }

    #[test]
    fn looks_like_html_artifact_detects_plain_html_tag() {
        assert!(looks_like_html_artifact(
            "<html lang=\"en\"><head></head><body>full page here</body></html>"
        ));
    }

    #[test]
    fn looks_like_html_artifact_detects_body_fragment() {
        assert!(looks_like_html_artifact(
            "<body><h1>Page</h1><p>content goes here</p></body>"
        ));
    }

    #[test]
    fn looks_like_html_artifact_ignores_short_text() {
        assert!(!looks_like_html_artifact("hello world"));
        assert!(!looks_like_html_artifact("<html>"));
        assert!(!looks_like_html_artifact(""));
    }

    #[test]
    fn looks_like_html_artifact_ignores_html_in_prose() {
        // A user saying "use <html> tags" is not an artifact: prose
        // never starts with `<`, so the heuristic rejects it.
        assert!(!looks_like_html_artifact(
            "Sure, I can use <html> tags in the response if you prefer."
        ));
    }

    #[test]
    fn looks_like_html_artifact_ignores_xml_declaration_only() {
        // Bare XML prolog without any HTML open tag is not reviewable.
        assert!(!looks_like_html_artifact(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>"
        ));
    }

    #[test]
    fn diagnostic_for_review_matches_prd_format() {
        let line = diagnostic_for_review("http://127.0.0.1:4321/");
        assert_eq!(
            line,
            "[VesperLens] Artifact ready for review. Open: http://127.0.0.1:4321/"
        );
    }

    #[test]
    fn feedback_context_message_for_approve_is_terse() {
        let fb = LensFeedback {
            action: Action::Approve,
            ..Default::default()
        };
        let msg = feedback_as_context_message(&fb);
        assert!(msg.contains("VesperLens human review: APPROVED"));
        assert!(!msg.contains("Overall notes:"));
        assert!(!msg.contains("Annotations"));
    }

    #[test]
    fn feedback_context_message_for_modify_includes_annotations() {
        let fb = LensFeedback {
            action: Action::Modify,
            annotations: vec![crate::planning::vesper_lens::DomAnnotation {
                selector: "#hero".into(),
                comment: "too big".into(),
                suggested_html: Some("<h1>smaller</h1>".into()),
            }],
            notes: "fix it".into(),
            answers: Vec::new(),
        };
        let msg = feedback_as_context_message(&fb);
        assert!(msg.contains("NEEDS MODIFICATION"));
        assert!(msg.contains("Overall notes: fix it"));
        assert!(msg.contains("Annotations (1):"));
        assert!(msg.contains("selector: #hero"));
        assert!(msg.contains("comment:  too big"));
        assert!(msg.contains("suggested: <h1>smaller</h1>"));
    }

    #[test]
    fn feedback_context_message_includes_structured_planning_answers() {
        let fb = LensFeedback {
            action: Action::Modify,
            answers: vec![
                crate::planning::LensAnswer {
                    question: "framework".into(),
                    value: "Rust".into(),
                },
                crate::planning::LensAnswer {
                    question: "targets".into(),
                    value: "Web, Desktop".into(),
                },
            ],
            ..Default::default()
        };
        let msg = feedback_as_context_message(&fb);
        assert!(msg.contains("Planning answers (2):"));
        assert!(msg.contains("framework: Rust"));
        assert!(msg.contains("targets: Web, Desktop"));
    }

    #[test]
    fn feedback_context_message_for_reject_with_notes() {
        let fb = LensFeedback {
            action: Action::Reject,
            notes: "wrong color scheme entirely".into(),
            ..Default::default()
        };
        let msg = feedback_as_context_message(&fb);
        assert!(msg.contains("REJECTED"));
        assert!(msg.contains("Overall notes: wrong color scheme entirely"));
        assert!(!msg.contains("Annotations"));
    }
}
