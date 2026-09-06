//! The browser action vocabulary (VRO-14 PR-4, gamma port).
//!
//! [`BrowserAction`] is the complete set of operations the Action Engine
//! can perform on a page. The enum is intentionally closed: every variant
//! maps to a bounded CDP method sequence in the driver, and the registry
//! ([`action_registry`]) gives hosts the model-facing schema surface
//! (name, description, mutating flag) without exposing CDP details.
//!
//! Index-targeted variants (`Click`, `Type`, `SelectOption`) reference the
//! **interactable index** produced by [`crate::selector_map`] — never a
//! CSS selector or an XPath. Indices are stable within a session for the
//! lifetime of the underlying `backend_node_id` (gamma's contract), so a
//! model may quote an index it saw in an earlier observation of the same
//! step.

use crate::interactable::redact;

/// One browser operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserAction {
    /// Load an absolute http(s) URL. The egress gate classifies the target
    /// exactly like a fetch (private ranges are refused by the driver).
    Navigate {
        /// Absolute URL.
        url: String,
    },
    /// Click the interactable element with this session index.
    Click {
        /// Index from the session's interactable map.
        index: usize,
    },
    /// Type text into the indexed field, replacing its value.
    Type {
        /// Index from the session's interactable map.
        index: usize,
        /// Text to enter. Values typed into sensitive fields are redacted
        /// in every observation the model sees; the driver never echoes
        /// them back.
        text: String,
        /// Clear the field before typing (default true).
        clear_first: bool,
    },
    /// Scroll the page or the indexed scrollable element.
    Scroll {
        /// Pixels down (negative scrolls up).
        dy: i32,
        /// `None` scrolls the main frame; `Some(i)` scrolls the indexed
        /// scrollable element.
        index: Option<usize>,
    },
    /// Select an option of the indexed `<select>` by value.
    SelectOption {
        /// Index from the session's interactable map.
        index: usize,
        /// The option's `value` attribute.
        value: String,
    },
    /// Browser history back.
    Back,
    /// Browser history forward.
    Forward,
    /// Reload the current page.
    Reload,
    /// Capture the viewport as a PNG (returned as base64 in the result,
    /// bounded by the output cap).
    Screenshot,
    /// Close the page and end the session.
    Close,
}

impl BrowserAction {
    /// The registry name of this action (stable, snake_case).
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Navigate { .. } => "navigate",
            Self::Click { .. } => "click",
            Self::Type { .. } => "type",
            Self::Scroll { .. } => "scroll",
            Self::SelectOption { .. } => "select_option",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::Reload => "reload",
            Self::Screenshot => "screenshot",
            Self::Close => "close",
        }
    }

    /// Whether the action mutates page state (vs. merely observing).
    #[must_use]
    pub fn is_mutating(&self) -> bool {
        !matches!(self, Self::Screenshot { .. })
    }

    /// A model-facing description that redacts sensitive payloads.
    ///
    /// `Type` into a field the caller marks sensitive renders the text as
    /// the `•` mask; the caller (driver/host) knows the field kind from
    /// the interactable map and passes `sensitive` accordingly.
    #[must_use]
    pub fn describe(&self, sensitive: bool) -> String {
        match self {
            Self::Navigate { url } => format!("navigate {url}"),
            Self::Click { index } => format!("click [{index}]"),
            Self::Type {
                index,
                text,
                clear_first,
            } => {
                let shown = if sensitive {
                    redact(text.chars().count())
                } else {
                    text.clone()
                };
                let prefix = if *clear_first { "type" } else { "append" };
                format!("{prefix} {shown} into [{index}]")
            }
            Self::Scroll { dy, index } => match index {
                Some(i) => format!("scroll [{i}] by {dy}px"),
                None => format!("scroll page by {dy}px"),
            },
            Self::SelectOption { index, value } => {
                format!("select {value:?} in [{index}]")
            }
            Self::Back => "history back".into(),
            Self::Forward => "history forward".into(),
            Self::Reload => "reload page".into(),
            Self::Screenshot => "screenshot viewport".into(),
            Self::Close => "close page".into(),
        }
    }
}

/// One registry row: the schema the host advertises for an action kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSpec {
    /// Stable action name.
    pub name: &'static str,
    /// One-line description for the model.
    pub description: &'static str,
    /// Whether it mutates page state.
    pub mutating: bool,
}

/// The full action registry (every [`BrowserAction`] variant, once).
#[must_use]
pub fn action_registry() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            name: "navigate",
            description: "Load an absolute http(s) URL in the page.",
            mutating: true,
        },
        ActionSpec {
            name: "click",
            description: "Click the interactable element with this index.",
            mutating: true,
        },
        ActionSpec {
            name: "type",
            description: "Type text into the indexed input field.",
            mutating: true,
        },
        ActionSpec {
            name: "scroll",
            description: "Scroll the page or the indexed scrollable element.",
            mutating: true,
        },
        ActionSpec {
            name: "select_option",
            description: "Choose an option of the indexed select element.",
            mutating: true,
        },
        ActionSpec {
            name: "back",
            description: "Go back in browser history.",
            mutating: true,
        },
        ActionSpec {
            name: "forward",
            description: "Go forward in browser history.",
            mutating: true,
        },
        ActionSpec {
            name: "reload",
            description: "Reload the current page.",
            mutating: true,
        },
        ActionSpec {
            name: "screenshot",
            description: "Capture the viewport as a PNG.",
            mutating: false,
        },
        ActionSpec {
            name: "close",
            description: "Close the page and end the session.",
            mutating: true,
        },
    ]
}

/// Outcome of executing one action (the observation the model sees next).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResult {
    /// The action's registry name, echoed.
    pub action: String,
    /// Model-facing description (already redacted where sensitive).
    pub description: String,
    /// The page's interactable map after the action (the next
    /// observation), when the page still exists.
    pub map: Option<String>,
    /// Base64 PNG for `Screenshot`; empty otherwise.
    pub screenshot_base64: String,
    /// Human-readable failure text when the action did not complete.
    pub error: Option<String>,
}

impl ActionResult {
    /// A successful non-screenshot result carrying the next map.
    #[must_use]
    pub fn ok(action: &BrowserAction, sensitive: bool, map: String) -> Self {
        Self {
            action: action.name().to_string(),
            description: action.describe(sensitive),
            map: Some(map),
            screenshot_base64: String::new(),
            error: None,
        }
    }

    /// A failed result; `map` may still carry the last good observation.
    #[must_use]
    pub fn err(
        action: &BrowserAction,
        sensitive: bool,
        reason: String,
        map: Option<String>,
    ) -> Self {
        Self {
            action: action.name().to_string(),
            description: action.describe(sensitive),
            map,
            screenshot_base64: String::new(),
            error: Some(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_every_variant_exactly_once() {
        let registry = action_registry();
        let names: Vec<_> = registry.iter().map(|spec| spec.name).collect();
        let expected = [
            "navigate",
            "click",
            "type",
            "scroll",
            "select_option",
            "back",
            "forward",
            "reload",
            "screenshot",
            "close",
        ];
        assert_eq!(names, expected);
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "no duplicate registry names");
    }

    #[test]
    fn every_variant_names_itself_consistently() {
        let samples = [
            BrowserAction::Navigate {
                url: "https://x.invalid/".into(),
            },
            BrowserAction::Click { index: 3 },
            BrowserAction::Type {
                index: 1,
                text: "hi".into(),
                clear_first: true,
            },
            BrowserAction::Scroll {
                dy: -400,
                index: None,
            },
            BrowserAction::SelectOption {
                index: 2,
                value: "a".into(),
            },
            BrowserAction::Back,
            BrowserAction::Forward,
            BrowserAction::Reload,
            BrowserAction::Screenshot,
            BrowserAction::Close,
        ];
        let registry = action_registry();
        for action in &samples {
            assert!(
                registry.iter().any(|spec| spec.name == action.name()),
                "{} missing from registry",
                action.name()
            );
        }
    }

    #[test]
    fn sensitive_typing_is_masked_in_descriptions() {
        let action = BrowserAction::Type {
            index: 4,
            text: "hunter2".into(),
            clear_first: true,
        };
        let described = action.describe(true);
        assert!(
            !described.contains("hunter2"),
            "sensitive text leaked: {described}"
        );
        assert!(described.contains("•••••••"), "mask missing: {described}");
        // Non-sensitive shows verbatim.
        assert!(action.describe(false).contains("hunter2"));
    }

    #[test]
    fn long_sensitive_values_are_capped_masks() {
        let action = BrowserAction::Type {
            index: 4,
            text: "a".repeat(40),
            clear_first: false,
        };
        let described = action.describe(true);
        let bullets = described.chars().filter(|c| *c == '•').count();
        assert!(bullets <= crate::interactable::REDACTION_MASK_MAX);
    }

    #[test]
    fn screenshot_is_the_only_non_mutating_action() {
        assert!(!BrowserAction::Screenshot.is_mutating());
        let others = [
            BrowserAction::Back,
            BrowserAction::Forward,
            BrowserAction::Reload,
            BrowserAction::Close,
            BrowserAction::Navigate { url: "u".into() },
        ];
        for action in others {
            assert!(action.is_mutating(), "{} must be mutating", action.name());
        }
    }
}
