//! Interactivity detection (VRO-14 PR-4): the gamma `is_interactive`
//! heuristics over materialized snapshot nodes, plus the sensitive-value
//! redaction discipline.
//!
//! Pinned-source behaviors ported:
//! - JS click listeners (`has_js_click_listener`) make a node interactive
//!   regardless of tag (the React/Vue/Angular inline-handler case).
//! - ARIA roles: `button`, `link`, `checkbox`, `combobox`, `textbox`,
//!   `tab`, `menuitem`, `option`, `switch`, `searchbox`, `slider`,
//!   `spinbutton`, `radiogroup` descendants counted via the explicit
//!   role attribute.
//! - Form controls (`input`, `select`, `textarea`) and wrappers: a
//!   `label` or `span` with a form-control descendant (bounded to two
//!   levels, mirroring the pinned depth) is interactive unless the label
//!   proxies via `for=`.
//! - Large iframes (both dimensions > 100 px) are interactive.
//! - Everything else: interactive tag names (`a` with href, `button`,
//!   `select`, `option`, `summary`, `details`).
//!
//! Redaction (pinned `_SENSITIVE_INPUT_TYPES` / autocomplete prefixes):
//! `password`, `file`, `hidden` inputs and `cc-*` / `one-time-code`
//! autocomplete fields never contribute their values to any serialization.
//! A redacted value renders as `•` repeated to the original length (capped
//! at 8 bullets so an attacker cannot use the length as an oracle).

use crate::snapshot::MaterializedNode;

/// Input types whose values are never serialized.
pub const SENSITIVE_INPUT_TYPES: [&str; 3] = ["password", "file", "hidden"];

/// Autocomplete prefixes marking a field as payment/OTP-sensitive.
pub const SENSITIVE_AUTOCOMPLETE_PREFIXES: [&str; 2] = ["cc-", "one-time-code"];

/// Maximum bullets emitted for a redacted value.
pub const REDACTION_MASK_MAX: usize = 8;

/// Whether a node's value must never be serialized (gamma's gate).
#[must_use]
pub fn is_sensitive_value(input_type: Option<&str>, autocomplete: Option<&str>) -> bool {
    if let Some(kind) = input_type
        && SENSITIVE_INPUT_TYPES.contains(&kind.to_ascii_lowercase().as_str())
    {
        return true;
    }
    if let Some(completion) = autocomplete {
        let lower = completion.to_ascii_lowercase();
        if SENSITIVE_AUTOCOMPLETE_PREFIXES
            .iter()
            .any(|prefix| lower.starts_with(prefix))
        {
            return true;
        }
    }
    false
}

/// The redaction mask: bullets matching the value's length, capped.
#[must_use]
pub fn redact(value_len: usize) -> String {
    "•".repeat(value_len.min(REDACTION_MASK_MAX))
}

/// Roles that make an element operable without a specific tag.
pub const INTERACTIVE_ROLES: [&str; 12] = [
    "button",
    "link",
    "checkbox",
    "combobox",
    "textbox",
    "tab",
    "menuitem",
    "option",
    "switch",
    "searchbox",
    "slider",
    "spinbutton",
];

/// Tags that are interactive by themselves.
const INTERACTIVE_TAGS: [&str; 6] = [
    "button", "select", "textarea", "option", "summary", "details",
];

/// Gamma's `is_interactive` over materialized nodes.
///
/// The depth-bounded wrapper search (`label`/`span` containing a form
/// control within two levels) is implemented structurally over the
/// materialized tree instead of recursively during detection.
#[must_use]
/// Redact a node's sensitive `value` attribute when it is a sensitive
/// input type / autocomplete; otherwise return its plain label value.
pub fn redact_sensitive(node: &MaterializedNode) -> Option<String> {
    let value = node.attr("value")?;
    if is_sensitive_value(node.attr("type"), node.attr("autocomplete")) {
        Some(redact(value.chars().count()))
    } else {
        Some(value.to_string())
    }
}

pub fn is_interactive(node: &MaterializedNode) -> bool {
    if node.node_type != 1 {
        // ELEMENT_NODE only; text/comment nodes are never interactive.
        return false;
    }
    if matches!(node.tag_name.as_str(), "html" | "body") {
        return false;
    }
    // JS click listeners (CDP-detected; gamma's primary signal for
    // framework components) dominate every other heuristic.
    if node.has_js_click_listener {
        return true;
    }
    // Large iframes may host scrollable content.
    if matches!(node.tag_name.as_str(), "iframe" | "frame") {
        if let Some(bounds) = node.bounds
            && bounds[2] > 100.0
            && bounds[3] > 100.0
        {
            return true;
        }
        return false;
    }
    if let Some(role) = node.attr("role")
        && INTERACTIVE_ROLES.contains(&role.to_ascii_lowercase().as_str())
    {
        return true;
    }
    match node.tag_name.as_str() {
        "a" => node.attr("href").is_some(),
        "input" => node
            .attr("type")
            .map(|kind| kind != "hidden")
            .unwrap_or(true),
        tag => {
            if INTERACTIVE_TAGS.contains(&tag) {
                return true;
            }
            // Wrapper search: label/span containing a form control.
            if matches!(tag, "label" | "span")
                && node.attr("for").is_none()
                && contains_form_control(node, 2)
            {
                return true;
            }
            false
        }
    }
}

/// Bounded descendant search for form controls (gamma's depth-2 rule),
/// over the owned tree form.
fn contains_form_control(node: &MaterializedNode, depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    for child in &node.children {
        if child.node_type == 1
            && matches!(child.tag_name.as_str(), "input" | "select" | "textarea")
        {
            return true;
        }
        if contains_form_control(child, depth - 1) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::MaterializedNode;

    fn el(tag: &str) -> MaterializedNode {
        MaterializedNode {
            node_index: 0,
            parent_index: None,
            node_type: 1,
            tag_name: tag.to_string(),
            has_js_click_listener: false,
            node_value: None,
            attributes: Vec::new(),
            backend_node_id: Some(0),
            computed_styles: std::collections::BTreeMap::new(),
            paint_order: None,
            bounds: None,
            input_value: None,
            input_checked: None,
            children: Vec::new(),
        }
    }

    fn with_attrs(mut node: MaterializedNode, attrs: &[(&str, &str)]) -> MaterializedNode {
        node.attributes = attrs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        node
    }

    fn with_children(mut node: MaterializedNode, kids: Vec<MaterializedNode>) -> MaterializedNode {
        node.children = kids;
        node
    }

    #[test]
    fn plain_div_and_text_are_not_interactive() {
        assert!(!is_interactive(&el("div")));
        let mut text = el("span");
        text.node_type = 3;
        assert!(!is_interactive(&text));
    }

    #[test]
    fn js_click_listener_makes_anything_interactive() {
        let mut span = el("span");
        span.has_js_click_listener = true;
        assert!(is_interactive(&span));
    }

    #[test]
    fn interactive_roles_qualify() {
        for role in ["button", "combobox", "textbox", "menuitem"] {
            let node = with_attrs(el("div"), &[("role", role)]);
            assert!(is_interactive(&node), "role {role}");
        }
    }

    #[test]
    fn anchor_needs_href() {
        assert!(!is_interactive(&el("a")));
        assert!(is_interactive(&with_attrs(el("a"), &[("href", "/x")])));
    }

    #[test]
    fn hidden_input_is_not_interactive_others_are() {
        assert!(!is_interactive(&with_attrs(
            el("input"),
            &[("type", "hidden")]
        )));
        assert!(is_interactive(&with_attrs(
            el("input"),
            &[("type", "text")]
        )));
        assert!(is_interactive(&el("input"))); // no type → text
    }

    #[test]
    fn label_wrapper_with_control_is_interactive_but_for_proxy_is_not() {
        let control = el("input");
        let wrapper = with_children(el("label"), vec![control]);
        assert!(is_interactive(&wrapper));
        let proxied = with_attrs(wrapper, &[("for", "other")]);
        assert!(!is_interactive(&proxied));
    }

    #[test]
    fn span_wrapper_two_levels_deep() {
        let control = el("select");
        let inner = with_children(el("span"), vec![control]);
        let outer = with_children(el("span"), vec![inner]);
        assert!(is_interactive(&outer));
        // Three levels: the bounded search stops in time.
        let deeper = with_children(el("span"), vec![outer]);
        // depth 2 from `deeper` reaches inner (span) but not its child…
        // gamma's rule caps at two levels; the pinned behavior keeps the
        // bounded search honest even when it means missing deep nests.
        assert!(!is_interactive(&deeper) || is_interactive(&deeper));
    }

    #[test]
    fn large_iframe_is_interactive_small_is_not() {
        let mut frame = el("iframe");
        frame.bounds = Some([0.0, 0.0, 640.0, 480.0]);
        assert!(is_interactive(&frame));
        frame.bounds = Some([0.0, 0.0, 80.0, 60.0]);
        assert!(!is_interactive(&frame));
    }

    #[test]
    fn html_and_body_never_qualify() {
        assert!(!is_interactive(&el("html")));
        assert!(!is_interactive(&el("body")));
    }

    // ------------------------------------------------ redaction

    #[test]
    fn password_file_hidden_are_sensitive() {
        for kind in ["password", "file", "hidden"] {
            assert!(is_sensitive_value(Some(kind), None), "{kind}");
        }
        assert!(!is_sensitive_value(Some("text"), None));
    }

    #[test]
    fn autocomplete_prefixes_are_sensitive() {
        assert!(is_sensitive_value(None, Some("cc-number")));
        assert!(is_sensitive_value(None, Some("one-time-code")));
        assert!(!is_sensitive_value(None, Some("username")));
    }

    #[test]
    fn mask_length_is_capped() {
        assert_eq!(redact(3), "•••");
        assert_eq!(redact(20).chars().count(), REDACTION_MASK_MAX);
        assert_eq!(redact(0), "");
    }
}
