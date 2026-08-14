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
    fn review(
        &self,
        html: &str,
        on_url: &(dyn Fn(&str) + Send + Sync),
    ) -> Pin<Box<dyn Future<Output = Result<LensFeedback, LensError>> + Send + '_>>;
}

/// Default no-op port. Used when VesperLens is not configured. Returns
/// `LensFeedback::default()` (action = [`Action::Reject`]) immediately so
/// the orchestrator's caller sees a benign "not reviewed" outcome without
/// ever attempting network I/O.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOpLensReviewPort;

impl LensReviewPort for NoOpLensReviewPort {
    fn review(
        &self,
        _html: &str,
        _on_url: &(dyn Fn(&str) + Send + Sync),
    ) -> Pin<Box<dyn Future<Output = Result<LensFeedback, LensError>> + Send + '_>> {
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

// ---------------------------------------------------------------------------
// VRO-11.3 directive 4 — write_file(.html) tool-observer interceptor
// ---------------------------------------------------------------------------

/// Heuristic: did this tool call write a self-contained HTML artifact that
/// should be routed through VesperLens human-in-the-loop review?
///
/// Triggers **only** when:
/// 1. `name == "write_file"` (the canonical mutating write tool), AND
/// 2. the `path` argument ends with `.html` (case-insensitive), AND
/// 3. the `content` argument parses as a string AND
///    [`looks_like_html_artifact`] agrees it is a reviewable artifact.
///
/// Returns the extracted content so the caller can pass it to
/// [`LensReviewPort::review`] without re-reading the file. Returns `None`
/// for every other combination so the React loop's tool result is
/// untouched (zero behavior change for non-HTML writes).
///
/// This is the file-save interceptor that closes the gap the VRO-11.3
/// directive diagnosed: previously VesperLens only triggered when the
/// agent's *final conversational text* started with `<html` — but the
/// agent typically writes the dashboard to disk via `write_file` and then
/// answers with a short prose summary, so the review oracle never fired.
/// Routing the review off the tool call itself catches the artifact at the
/// moment it lands on disk.
#[must_use]
pub fn html_artifact_for_write_file(name: &str, arguments: &serde_json::Value) -> Option<String> {
    if name != "write_file" {
        return None;
    }
    let path = arguments.get("path").and_then(|v| v.as_str())?;
    if !path.to_ascii_lowercase().ends_with(".html") {
        return None;
    }
    let content = arguments.get("content").and_then(|v| v.as_str())?;
    if !looks_like_html_artifact(content) {
        return None;
    }
    Some(content.to_string())
}

/// A [`ToolInvoker`] decorator that routes successful `write_file(.html)`
/// calls through the configured [`LensReviewPort`] for human-in-the-loop
/// review (VRO-11.3 directive 4).
///
/// Wraps any inner invoker (typically the production
/// [`RegistryToolInvoker`](crate::vro::react::RegistryToolInvoker)). After
/// every successful `invoke`, [`html_artifact_for_write_file`] decides
/// whether the call wrote a reviewable HTML artifact; if so, the lens port
/// is invoked synchronously (the React loop **halts** until the human
/// submits feedback — this is the directive's "halt for human review"
/// behavior).
///
/// When the human returns [`Action::Approve`], the original tool result is
/// returned unchanged. When the human returns [`Action::Reject`] or
/// [`Action::Modify`], the feedback is appended to the tool result so the
/// model's next ReAct step can react to the verdict (the standard
/// `role: Tool` context-injection pattern from
/// [`feedback_as_context_message`]).
///
/// Zero behavior change when the wrapped invoker is used without a lens
/// port — the decorator is only constructed by
/// [`VroOrchestrator::execute_react`](crate::vro::VroOrchestrator::execute_react)
/// when `lens_port` is `Some`.
pub struct LensObservingInvoker<'a> {
    inner: &'a dyn crate::vro::react::ToolInvoker,
    lens: &'a dyn LensReviewPort,
    on_diagnostic: &'a (dyn Fn(&str) + Send + Sync),
}

impl<'a> std::fmt::Debug for LensObservingInvoker<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LensObservingInvoker")
            .field("inner", &(self.inner as *const _))
            .field("lens", &(self.lens as *const _))
            .finish_non_exhaustive()
    }
}

impl<'a> LensObservingInvoker<'a> {
    /// Wraps `inner` so successful `write_file(.html)` calls are routed
    /// through `lens` for human review. `on_diagnostic` receives the
    /// formatted `[VesperLens] Artifact ready for review. Open: <URL>`
    /// line right before the port blocks awaiting the human; the host
    /// wires this to its status line / log sink.
    #[must_use]
    pub fn new(
        inner: &'a dyn crate::vro::react::ToolInvoker,
        lens: &'a dyn LensReviewPort,
        on_diagnostic: &'a (dyn Fn(&str) + Send + Sync),
    ) -> Self {
        Self {
            inner,
            lens,
            on_diagnostic,
        }
    }
}

impl<'a> crate::vro::react::ToolInvoker for LensObservingInvoker<'a> {
    fn class_of(&self, name: &str) -> Option<vesper_domain::ToolExecutionClass> {
        self.inner.class_of(name)
    }

    fn invoke<'b>(
        &'b self,
        name: &'b str,
        arguments: &'b serde_json::Value,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<String, crate::vro::react::ToolInvocationError>>
                + Send
                + 'b,
        >,
    > {
        let inner = self.inner;
        let lens = self.lens;
        let on_diagnostic = self.on_diagnostic;
        Box::pin(async move {
            let result = inner.invoke(name, arguments).await;
            // Directive 4: only intercept successful write_file(.html)
            // calls. Errors and every other tool pass through unchanged.
            if let Ok(text) = &result
                && let Some(html) = html_artifact_for_write_file(name, arguments)
            {
                let on_url = |url: &str| {
                    on_diagnostic(&diagnostic_for_review(url));
                };
                // The port blocks until the human submits (or the
                // VesperLens timeout fires). This is the directive's
                // "halt for human review" semantics — the React loop
                // pauses mid-turn.
                if let Ok(feedback) = lens.review(&html, &on_url).await {
                    // Approve → original result unchanged.
                    // Reject / Modify → append the verdict so the
                    // model's next step can react to the human's
                    // corrections.
                    if feedback.action != Action::Approve {
                        let mut augmented = String::with_capacity(text.len() + 256);
                        augmented.push_str(text);
                        augmented.push_str("\n\n");
                        augmented.push_str(&feedback_as_context_message(&feedback));
                        return Ok(augmented);
                    }
                }
                // Review failed (timeout / parse error / disconnected)
                // OR human approved — return the original result.
            }
            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vro::react::ToolInvoker;
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
        fn review(
            &self,
            html: &str,
            on_url: &(dyn Fn(&str) + Send + Sync),
        ) -> Pin<Box<dyn Future<Output = Result<LensFeedback, LensError>> + Send + '_>> {
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

    // -----------------------------------------------------------------
    // VRO-11.3 directive 4 — write_file(.html) interceptor
    // -----------------------------------------------------------------

    #[test]
    fn html_artifact_for_write_file_extracts_content_from_html_path() {
        let args = serde_json::json!({
            "path": "dashboard/index.html",
            "content": "<!doctype html><html><body><h1>Dashboard</h1></body></html>",
        });
        let extracted = html_artifact_for_write_file("write_file", &args);
        assert_eq!(
            extracted.as_deref(),
            Some("<!doctype html><html><body><h1>Dashboard</h1></body></html>")
        );
    }

    #[test]
    fn html_artifact_for_write_file_is_case_insensitive_on_extension() {
        let args = serde_json::json!({
            "path": "out/Report.HTML",
            "content": "<html><body>long enough body content goes here</body></html>",
        });
        assert!(html_artifact_for_write_file("write_file", &args).is_some());
    }

    #[test]
    fn html_artifact_for_write_file_ignores_non_html_extensions() {
        let args = serde_json::json!({
            "path": "src/main.rs",
            "content": "<html><body>not really html, it's source code</body></html>",
        });
        assert!(html_artifact_for_write_file("write_file", &args).is_none());
    }

    #[test]
    fn html_artifact_for_write_file_ignores_non_write_file_tools() {
        let args = serde_json::json!({
            "path": "out/index.html",
            "content": "<html><body>content here</body></html>",
        });
        assert!(html_artifact_for_write_file("read_file", &args).is_none());
        assert!(html_artifact_for_write_file("edit_file", &args).is_none());
        assert!(html_artifact_for_write_file("bash", &args).is_none());
    }

    #[test]
    fn html_artifact_for_write_file_ignores_short_or_prose_content() {
        // The content still has to pass looks_like_html_artifact — short
        // fragments and prose mentions are NOT interceptable.
        let args = serde_json::json!({
            "path": "out/short.html",
            "content": "<html>",
        });
        assert!(html_artifact_for_write_file("write_file", &args).is_none());

        let args = serde_json::json!({
            "path": "out/prose.html",
            "content": "Sure, I can use <html> tags if you prefer.",
        });
        assert!(html_artifact_for_write_file("write_file", &args).is_none());
    }

    #[test]
    fn html_artifact_for_write_file_ignores_missing_fields() {
        // Missing path → None.
        let args = serde_json::json!({"content": "<html><body>x</body></html>"});
        assert!(html_artifact_for_write_file("write_file", &args).is_none());
        // Missing content → None.
        let args = serde_json::json!({"path": "out/x.html"});
        assert!(html_artifact_for_write_file("write_file", &args).is_none());
        // Non-string fields → None.
        let args = serde_json::json!({"path": 42, "content": "<html></html>"});
        assert!(html_artifact_for_write_file("write_file", &args).is_none());
    }

    /// A fake inner invoker that returns a fixed result for any call.
    struct FixedInvoker {
        result: Result<String, crate::vro::react::ToolInvocationError>,
    }

    impl crate::vro::react::ToolInvoker for FixedInvoker {
        fn class_of(&self, _name: &str) -> Option<vesper_domain::ToolExecutionClass> {
            None
        }
        fn invoke<'a>(
            &'a self,
            _name: &'a str,
            _args: &'a serde_json::Value,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<String, crate::vro::react::ToolInvocationError>,
                    > + Send
                    + 'a,
            >,
        > {
            let result = self.result.clone();
            Box::pin(async move { result })
        }
    }

    fn silent_diagnostic() -> Box<dyn Fn(&str) + Send + Sync> {
        Box::new(|_line: &str| {})
    }

    #[tokio::test]
    async fn lens_observing_invoker_passes_non_html_writes_unchanged() {
        // A .rs write must pass through with no lens invocation.
        let lens = RecordingLens::new(LensFeedback::default());
        let inner = FixedInvoker {
            result: Ok("wrote 1234 bytes".to_string()),
        };
        let diag = silent_diagnostic();
        let wrapper = LensObservingInvoker::new(&inner, &lens, diag.as_ref());
        let args = serde_json::json!({
            "path": "src/main.rs",
            "content": "fn main() {}",
        });
        let result = wrapper.invoke("write_file", &args).await.unwrap();
        assert_eq!(result, "wrote 1234 bytes");
        // Lens MUST NOT have been called for a non-HTML write.
        assert!(lens.captured_html.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn lens_observing_invoker_skips_review_on_tool_failure() {
        // A failed write_file(.html) must NOT trigger review — the file
        // wasn't actually written, so there is nothing for the human to
        // approve.
        let lens = RecordingLens::new(LensFeedback::default());
        let inner_err =
            crate::vro::react::ToolInvocationError::ExecutionFailed("disk full".to_string());
        let inner = FixedInvoker {
            result: Err(inner_err),
        };
        let diag = silent_diagnostic();
        let wrapper = LensObservingInvoker::new(&inner, &lens, diag.as_ref());
        let args = serde_json::json!({
            "path": "out/dashboard.html",
            "content": "<!doctype html><html><body>dashboard content here</body></html>",
        });
        let result = wrapper.invoke("write_file", &args).await;
        assert!(result.is_err());
        assert!(lens.captured_html.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn lens_observing_invoker_approve_returns_original_result_unchanged() {
        // Human approves → original tool result returned verbatim, no
        // appended feedback (noise-free success path).
        let lens = RecordingLens::new(LensFeedback {
            action: Action::Approve,
            ..Default::default()
        });
        let inner = FixedInvoker {
            result: Ok("wrote 4096 bytes".to_string()),
        };
        let diag = silent_diagnostic();
        let wrapper = LensObservingInvoker::new(&inner, &lens, diag.as_ref());
        let args = serde_json::json!({
            "path": "out/dashboard.html",
            "content": "<!doctype html><html><body>dashboard content here</body></html>",
        });
        let result = wrapper.invoke("write_file", &args).await.unwrap();
        assert_eq!(result, "wrote 4096 bytes");
        // Lens WAS called once.
        assert_eq!(lens.captured_html.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn lens_observing_invoker_reject_appends_feedback_to_result() {
        // Human rejects → the verdict is appended to the tool result so
        // the model's next ReAct step can react. The original tool result
        // is preserved at the start of the string.
        let lens = RecordingLens::new(LensFeedback {
            action: Action::Reject,
            notes: "the hero section is too aggressive".to_string(),
            ..Default::default()
        });
        let inner = FixedInvoker {
            result: Ok("wrote 8192 bytes".to_string()),
        };
        let diag = silent_diagnostic();
        let wrapper = LensObservingInvoker::new(&inner, &lens, diag.as_ref());
        let args = serde_json::json!({
            "path": "out/dashboard.html",
            "content": "<!doctype html><html><body><h1>dashboard content goes here</h1></body></html>",
        });
        let result = wrapper.invoke("write_file", &args).await.unwrap();
        // Original tool result preserved at the start.
        assert!(
            result.starts_with("wrote 8192 bytes"),
            "result should preserve the original tool output: {result}"
        );
        // Feedback appended.
        assert!(result.contains("REJECTED"), "got: {result}");
        assert!(
            result.contains("the hero section is too aggressive"),
            "got: {result}"
        );
    }

    #[tokio::test]
    async fn lens_observing_invoker_class_of_delegates_to_inner() {
        // The decorator must not invent its own classification —
        // Read-Before-Write policy depends on the inner invoker's view.
        struct ClassifiedInvoker;
        impl crate::vro::react::ToolInvoker for ClassifiedInvoker {
            fn class_of(&self, name: &str) -> Option<vesper_domain::ToolExecutionClass> {
                use vesper_domain::ToolExecutionClass;
                if name == "write_file" {
                    Some(ToolExecutionClass::Mutating)
                } else {
                    None
                }
            }
            fn invoke<'a>(
                &'a self,
                _name: &'a str,
                _args: &'a serde_json::Value,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<String, crate::vro::react::ToolInvocationError>,
                        > + Send
                        + 'a,
                >,
            > {
                Box::pin(async { Ok(String::new()) })
            }
        }
        let lens = RecordingLens::new(LensFeedback::default());
        let inner = ClassifiedInvoker;
        let diag = silent_diagnostic();
        let wrapper = LensObservingInvoker::new(&inner, &lens, diag.as_ref());
        assert_eq!(
            wrapper.class_of("write_file"),
            Some(vesper_domain::ToolExecutionClass::Mutating)
        );
        assert_eq!(wrapper.class_of("unknown"), None);
    }
}
