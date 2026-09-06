//! BrowserDriverPort — the Action Engine's headless-browser seam
//! (VRO-14 PR-4, gamma port).
//!
//! Contract (PRD Feature 2 + this PR's TCP ban):
//!
//! - The driver spawns the headless browser **inside the sandbox** with
//!   `--remote-debugging-pipe` and `--no-sandbox --disable-gpu`; it never
//!   passes `--remote-debugging-port`, never binds a socket, and never
//!   talks TCP. The only CDP channel is the anonymous-pipe pair the PR-0
//!   spike validated (fd 3 command read end, fd 4 event write end), which
//!   the sandbox backend maps for us the same way it maps the fetch
//!   helper's stdio.
//! - Capabilities are checked fail-closed before any spawn: a backend
//!   that cannot satisfy `IsolationRequirement::Network` yields the
//!   explicit refusal, never an unsandboxed browser.
//! - Every action maps to a bounded CDP command sequence; the driver
//!   surfaces typed failures and keeps its per-connection message ids
//!   monotonic so interleaved responses cannot alias.
//!
//! This module defines the port and the pure command-shaping layer; the
//! raw pipe protocol (NUL-delimited JSON) is shared with the PR-0 spike
//! and unit-tested against recorded transcripts.

use crate::action::{ActionResult, BrowserAction};
use crate::transport::BoxFuture;
use serde_json::{Value, json};

/// One CDP protocol message (command or event), NUL-delimited on the wire.
#[derive(Debug, Clone, PartialEq)]
pub struct CdpMessage(pub Value);

/// The browser-driver seam. Hosts inject the sandbox-backed
/// implementation at the composition boundary.
pub trait BrowserDriverPort: Send + Sync {
    /// Execute one action against the current page.
    fn execute(&self, action: &BrowserAction) -> BoxFuture<'_, Result<ActionResult, DriverError>>;
    /// Optional form submission extension; implementations must not silently
    /// ignore a requested submit.
    fn execute_with_submit(
        &self,
        action: &BrowserAction,
        submit: bool,
    ) -> BoxFuture<'_, Result<ActionResult, DriverError>> {
        if submit {
            return Box::pin(async {
                Err(DriverError::Invalid(
                    "driver does not support submit".into(),
                ))
            });
        }
        self.execute(action)
    }
    /// Terminate the browser session (idempotent).
    fn close(&self) -> BoxFuture<'_, Result<(), DriverError>>;
}

/// Typed driver failures (model-facing text in `Display`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriverError {
    /// The sandbox backend cannot isolate the browser.
    #[error("browser sandbox unavailable: {0} (refused rather than run unsandboxed)")]
    Sandbox(String),
    /// The browser process or pipe failed.
    #[error("browser pipe failed: {0}")]
    Pipe(String),
    /// CDP answered an error for a command.
    #[error("cdp rejected {method}: {message}")]
    Cdp {
        /// The rejected method name.
        method: String,
        /// The protocol error text.
        message: String,
    },
    /// The action referenced an index this session never assigned.
    #[error("unknown interactable index {0} in this session")]
    UnknownIndex(usize),
    /// The action was structurally invalid (empty text, bad selector).
    #[error("invalid action: {0}")]
    Invalid(String),
}

/// Diagnostic action outline, NOT an executable CDP wire sequence.
///
/// Index hints deliberately remain unresolved in this pure module. Production
/// execution lives in `vesper-web-fetch::browser::BrowserSession`, which
/// resolves live backend nodes and supplies complete protocol parameters.
/// Retained for action-shape compatibility tests only; never send this output
/// to a browser.
pub fn plan_commands(action: &BrowserAction, session_id: &str) -> Vec<(String, Value)> {
    match action {
        BrowserAction::Navigate { url } => vec![(
            "Page.navigate".into(),
            session_json(session_id, json!({ "url": url })),
        )],
        BrowserAction::Click { index } => {
            // Resolve the index to a backend node via the session's
            // selector map, then dispatch a real mouse event at its
            // center (gamma's approach: synthesized input, not JS .click()).
            vec![
                (
                    "DOM.resolveNode".into(),
                    session_json(session_id, json!({ "index_hint": index })),
                ),
                (
                    "Input.dispatchMouseEvent".into(),
                    session_json(
                        session_id,
                        json!({ "type": "mousePressed", "button": "left", "clickCount": 1 }),
                    ),
                ),
                (
                    "Input.dispatchMouseEvent".into(),
                    session_json(
                        session_id,
                        json!({ "type": "mouseReleased", "button": "left", "clickCount": 1 }),
                    ),
                ),
            ]
        }
        BrowserAction::Type { index, text, .. } => vec![(
            "Input.insertText".into(),
            session_json(session_id, json!({ "index_hint": index, "text": text })),
        )],
        BrowserAction::Scroll { dy, index } => match index {
            None => vec![(
                "Input.dispatchMouseEvent".into(),
                session_json(session_id, json!({ "type": "mouseWheel", "deltaY": dy })),
            )],
            Some(scroll_index) => vec![(
                "Runtime.evaluate".into(),
                session_json(
                    session_id,
                    json!({ "expression": format!("window.scrollBy(0,{dy})"), "index_hint": scroll_index }),
                ),
            )],
        },
        BrowserAction::SelectOption { index, value } => vec![(
            "DOM.setAttributeValue".into(),
            session_json(
                session_id,
                json!({ "index_hint": index, "name": "value", "value": value }),
            ),
        )],
        BrowserAction::Back | BrowserAction::Forward | BrowserAction::Reload => {
            let method = match action {
                BrowserAction::Back => "Page.navigateToHistoryEntry",
                BrowserAction::Forward => "Page.navigateToHistoryEntry",
                _ => "Page.reload",
            };
            vec![(method.into(), session_json(session_id, json!({})))]
        }
        BrowserAction::Screenshot => vec![(
            "Page.captureScreenshot".into(),
            session_json(session_id, json!({ "format": "png" })),
        )],
        BrowserAction::Close => vec![("Browser.close".into(), json!({}))],
    }
}

fn session_json(session_id: &str, params: Value) -> Value {
    let mut value = json!({ "params": params });
    value["sessionId"] = Value::String(session_id.to_string());
    value
}

/// NUL-framing helpers shared with the PR-0 spike protocol.
pub mod framing {
    use serde_json::Value;

    /// Encode one message for the pipe: compact JSON + NUL.
    pub fn encode(message: &Value) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(message).unwrap_or_default();
        bytes.push(0);
        bytes
    }

    /// Extract complete NUL-terminated messages from a byte buffer,
    /// leaving any trailing partial message in place. Returns
    /// (messages, consumed_bytes).
    pub fn drain(buffer: &[u8]) -> (Vec<Value>, usize) {
        let mut messages = Vec::new();
        let mut consumed = 0;
        while let Some(pos) = buffer[consumed..].iter().position(|b| *b == 0) {
            let end = consumed + pos;
            if let Ok(value) = serde_json::from_slice(&buffer[consumed..end]) {
                messages.push(value);
            }
            consumed = end + 1;
        }
        (messages, consumed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "sess-1";

    #[test]
    fn navigate_maps_to_page_navigate_with_session() {
        let plan = plan_commands(
            &BrowserAction::Navigate {
                url: "https://example.com/".into(),
            },
            SESSION,
        );
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].0, "Page.navigate");
        assert_eq!(plan[0].1["sessionId"], SESSION);
        assert_eq!(plan[0].1["params"]["url"], "https://example.com/");
    }

    #[test]
    fn click_is_a_press_release_pair_on_input_domain() {
        let plan = plan_commands(&BrowserAction::Click { index: 3 }, SESSION);
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].0, "DOM.resolveNode");
        assert_eq!(plan[1].0, "Input.dispatchMouseEvent");
        assert_eq!(plan[2].0, "Input.dispatchMouseEvent");
        assert_eq!(plan[1].1["params"]["type"], "mousePressed");
        assert_eq!(plan[2].1["params"]["type"], "mouseReleased");
    }

    #[test]
    fn type_uses_insert_text() {
        let plan = plan_commands(
            &BrowserAction::Type {
                index: 1,
                text: "hello".into(),
                clear_first: true,
            },
            SESSION,
        );
        assert_eq!(plan[0].0, "Input.insertText");
        assert_eq!(plan[0].1["params"]["text"], "hello");
    }

    #[test]
    fn scroll_defaults_to_600_pixels() {
        let plan = plan_commands(
            &BrowserAction::Scroll {
                dy: 600,
                index: None,
            },
            SESSION,
        );
        assert_eq!(plan[0].0, "Input.dispatchMouseEvent");
        assert_eq!(plan[0].1["params"]["type"], "mouseWheel");
        assert_eq!(plan[0].1["params"]["deltaY"], 600);
    }

    #[test]
    fn screenshot_and_close_shapes() {
        assert_eq!(
            plan_commands(&BrowserAction::Screenshot, SESSION)[0].0,
            "Page.captureScreenshot"
        );
        let close = plan_commands(&BrowserAction::Close, SESSION);
        assert_eq!(close[0].0, "Browser.close");
        // Close is browser-scoped: no sessionId.
        assert!(close[0].1.get("sessionId").is_none());
    }

    #[test]
    fn framing_round_trips_multiple_messages() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&framing::encode(&json!({ "id": 1 })));
        buffer.extend_from_slice(&framing::encode(&json!({ "id": 2, "method": "x" })));
        buffer.extend_from_slice(b"{\"partial\":"); // no NUL yet
        let (messages, consumed) = framing::drain(&buffer);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1]["method"], "x");
        assert_eq!(consumed, buffer.len() - b"{\"partial\":".len());
    }

    #[test]
    fn framing_ignores_unparseable_segments() {
        let mut buffer = b"not json\x00".to_vec();
        buffer.extend_from_slice(&framing::encode(&json!({ "id": 9 })));
        let (messages, _) = framing::drain(&buffer);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["id"], 9);
    }

    #[test]
    fn driver_errors_render_model_facing_text() {
        let error = DriverError::Cdp {
            method: "Page.navigate".into(),
            message: "net::ERR_NAME_NOT_RESOLVED".into(),
        };
        let text = error.to_string();
        assert!(text.contains("Page.navigate"));
        assert!(text.contains("ERR_NAME_NOT_RESOLVED"));
        let refusal = DriverError::Sandbox("network unavailable".into()).to_string();
        assert!(refusal.contains("refused rather than run unsandboxed"));
    }
}
