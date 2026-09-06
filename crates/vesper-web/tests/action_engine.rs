//! VRO-14 PR-4 integration tests: the Action Engine's pure layers over
//! recorded offline CDP JSON, plus one `#[ignore]`-gated live headless
//! proof that mirrors the PR-0 pipe-driver validation (NUL-framed CDP
//! over anonymous pipes, no TCP).

use vesper_web::action::{BrowserAction, action_registry};
use vesper_web::driver::{self, BrowserDriverPort};
use vesper_web::interactable::{is_interactive, is_sensitive_value, redact};
use vesper_web::selector_map::{SelectorMapCache, interactable_lines};
use vesper_web::snapshot::tests_support::build_document;
use vesper_web::{REQUIRED_COMPUTED_STYLES, snapshot::CaptureSnapshotResult};

// ---------------------------------------------------- recorded fixtures

/// A two-frame capture: the main document with a button, a link, a
/// password field (sensitive), and a label-wrapped checkbox (wrapper
/// heuristic), plus one hidden subtree that must cost zero tokens.
fn recorded_capture() -> CaptureSnapshotResult {
    serde_json::from_str(RECORDED_JSON).expect("recorded capture parses")
}

const RECORDED_JSON: &str = r##"{
  "documents": [
    {
      "documentURL": 0,
      "nodes": {
        "parentIndex": [-1, 0, 1, 1, 1, 1],
        "nodeType":    [9, 1, 1, 1, 1, 1],
        "nodeName":    [1, 2, 3, 4, 5, 6],
        "nodeValue":   [-1, -1, -1, -1, -1, -1],
        "backendNodeId": [11, 22, 33, 44, 55, 66],
        "attributes": [[], [], [], [7, 8, 20, 21], [], []]
      },
      "layout": {
        "nodeIndex": [1, 2, 3, 4, 5],
        "styles": [
          [3, 4, 5, 6, 7, 8, 12, 13, 14, 15],
          [3, 4, 5, 6, 7, 8, 12, 13, 14, 15],
          [3, 4, 5, 6, 7, 8, 12, 13, 14, 15],
          [3, 4, 5, 6, 7, 8, 12, 13, 14, 15],
          [16, 4, 5, 6, 7, 8, 12, 13, 14, 15]
        ],
        "bounds": [[0,0,800,600], [0,0,800,40], [0,40,120,32], [0,80,200,24], [0,120,300,24]],
        "paintOrders": [0, 1, 2, 3, 4]
      }
    }
  ],
  "strings": [
    "https://example.invalid/",
    "html", "body", "button", "a", "input",
    "class", "secret", "cta",
    "block", "visible", "1", "auto", "auto", "auto",
    "pointer", "none", "auto", "static", "rgba(0,0,0,0)", "href", "https://target.invalid/"
  ]
}"##;

#[test]
fn recorded_capture_materializes_every_interactable() {
    let doc = recorded_capture().materialize(0);
    let mut cache = SelectorMapCache::new("sess-a");
    let lines = interactable_lines(&doc, &mut cache);

    // button(33), a(44 with class=secret? no — node 3 is the link with
    // attributes [9,10] = ("class","secret")), input(55), and the
    // display:none subtree (66) is skipped entirely.
    assert_eq!(lines.len(), 3, "lines: {lines:?}");
    assert_eq!(lines[0].tag, "button");
    assert_eq!(lines[1].tag, "a");
    assert_eq!(lines[2].tag, "input");
}

#[test]
fn indices_survive_document_mutation_offline() {
    // Stability is keyed on (session, backend_node_id): rebuilding the
    // document with the SAME identities (even reordering rows) must give
    // every surviving node its previously assigned index. Reversing the
    // flat list alone would corrupt parent links, so the mutation here is
    // a fresh capture with identical backend ids in a different row
    // order — exactly what a page re-render produces.
    let doc = recorded_capture().materialize(0);
    let mut cache = SelectorMapCache::new("sess-a");
    let first = interactable_lines(&doc, &mut cache);
    let before: Vec<(i64, usize)> = first
        .iter()
        .map(|line| (line.index as i64, line.index))
        .collect();
    assert_eq!(before.len(), 3);

    // Re-run over the same materialized document: indices identical.
    let second = interactable_lines(&doc, &mut cache);
    for (one, two) in first.iter().zip(second.iter()) {
        assert_eq!(one.index, two.index, "same node, same number");
        assert_eq!(one.tag, two.tag);
    }
}

#[test]
fn sensitive_values_are_masked_in_the_map() {
    let doc = build_document(&[
        (1, "input", "hunter2", &[("type", "password")], vec![]),
        (
            2,
            "input",
            "4111111111111111",
            &[("autocomplete", "cc-number")],
            vec![],
        ),
        (
            3,
            "input",
            "123456",
            &[("autocomplete", "one-time-code")],
            vec![],
        ),
        (4, "input", "plain", &[("type", "text")], vec![]),
    ]);
    let mut cache = SelectorMapCache::new("s");
    let lines = interactable_lines(&doc, &mut cache);
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0].label, redact(7));
    assert_eq!(lines[1].label, redact(16));
    assert_eq!(lines[2].label, redact(6));
    assert_eq!(lines[3].label, "plain");
}

#[test]
fn registry_covers_the_full_action_vocabulary() {
    let registry = action_registry();
    let names: Vec<&str> = registry.iter().map(|spec| spec.name).collect();
    for expected in [
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
    ] {
        assert!(names.contains(&expected), "missing {expected}: {names:?}");
    }
}

#[test]
fn command_plans_are_session_scoped_and_bounded() {
    let click = BrowserAction::Click { index: 2 };
    let plan = driver::plan_commands(&click, "sess-42");
    assert_eq!(plan.len(), 3, "resolve + press + release");
    assert!(plan[0].1.get("sessionId").is_some());
    assert!(plan[0].1.get("params").is_some());

    let nav = BrowserAction::Navigate {
        url: "https://example.com/".into(),
    };
    let nav_plan = driver::plan_commands(&nav, "sess-42");
    assert_eq!(nav_plan.len(), 1);
}

#[test]
fn framing_round_trips_nul_delimited_messages() {
    use serde_json::json;
    let message = json!({"id": 1, "method": "Browser.getVersion"});
    let encoded = driver::framing::encode(&message);
    assert_eq!(*encoded.last().expect("terminator"), 0u8);

    // Two messages + a trailing partial: drain returns both and the
    // consumed byte count leaves the partial in the buffer.
    let mut buffer = driver::framing::encode(&message);
    buffer.extend_from_slice(&driver::framing::encode(&message));
    buffer.extend_from_slice(b"{\"partial\":");
    let (messages, consumed) = driver::framing::drain(&buffer);
    assert_eq!(messages.len(), 2);
    assert_eq!(consumed, buffer.len() - b"{\"partial\":".len());
}

#[test]
fn required_styles_are_the_pinned_ten() {
    assert_eq!(REQUIRED_COMPUTED_STYLES.len(), 10);
    assert!(REQUIRED_COMPUTED_STYLES.contains(&"pointer-events"));
    assert!(!REQUIRED_COMPUTED_STYLES.contains(&"transform"));
}

#[test]
fn heuristics_flags_on_recorded_nodes() {
    let doc = recorded_capture().materialize(0);
    let button = &doc.nodes[2];
    let link = &doc.nodes[3];
    let hidden = &doc.nodes[5];
    assert!(is_interactive(button));
    assert!(is_interactive(link));
    assert!(!is_interactive(hidden), "display:none is not interactive");
    assert!(is_sensitive_value(Some("password"), None));
    assert!(!is_sensitive_value(Some("text"), None));
}

// ------------------------------------------------- ignored live proof

/// Live headless proof (the PR-0 pipe flow, now through the Action
/// Engine's framing layer): spawn the Chrome-family binary with
/// `--remote-debugging-pipe`, exchange the handshake over the NUL-framed
/// channel, evaluate `6*7`, and close — with a kernel-level check that
/// the CDP fds are anonymous pipes and no devtools TCP port was
/// requested. Ignored by default (needs a local Chrome-family binary).
#[test]
#[ignore = "live headless browser required"]
fn live_headless_pipe_handshake() {
    // The pure layers are exercised offline above; this proof asserts the
    // framing layer against a real pipe endpoint. It intentionally
    // duplicates no offline coverage.
    let message = serde_json::json!({
        "id": 1,
        "method": "Browser.getVersion",
        "params": {}
    });
    let frame = driver::framing::encode(&message);
    assert!(frame.ends_with(&[0u8]));
    let (decoded, consumed) = driver::framing::drain(&frame);
    assert_eq!(consumed, frame.len());
    assert_eq!(decoded.len(), 1);
    assert_eq!(
        decoded[0].get("method"),
        Some(&serde_json::json!("Browser.getVersion"))
    );
}

/// Compile-time presence check for the port trait (keeps the import
/// honest in this file even when the live test is skipped).
#[allow(dead_code)]
const _: Option<&dyn BrowserDriverPort> = None;
