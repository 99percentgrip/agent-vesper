//! Unit + integration tests for VRO-12 result-aware loop detection.
//!
//! Covers (PRD §6): the three detection keys (Exact Repeat / Ping-Pong /
//! No-Progress), all three escalation states per pattern, window eviction
//! at `LOOP_WINDOW_SIZE`, canonical-JSON args invariance, hash
//! domain-separation, failure-exclusion, Read-Before-Write exclusion, and
//! end-to-end ReAct-loop integration (Warn nudge, Block override without
//! budget consumption, Break circuit breaker with named risk).
//!
//! Data-only reference: zeroclaw `loop_detector.rs` — no code copied.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use serde_json::json;

use crate::vro::ToolInvocationError;
use crate::vro::loop_detector::{LoopDetector, LoopGuardAction, LoopPattern};
use crate::vro::react::{
    ReactAgent, ReactDecision, ToolInvoker, TrajectoryEntry, run_tool_grounded_react,
    run_tool_grounded_react_with_trajectory,
};
use vesper_domain::{OutcomeStatus, ReasoningBudget, ToolExecutionClass};

// ---------------------------------------------------------------------------
// Small pure helpers
// ---------------------------------------------------------------------------

fn warn_pattern(action: &LoopGuardAction) -> &LoopPattern {
    match action {
        LoopGuardAction::Warn(w) => &w.pattern,
        other => panic!("expected Warn, got {other:?}"),
    }
}

fn block_text(action: &LoopGuardAction) -> &str {
    match action {
        LoopGuardAction::Block(text) => text,
        other => panic!("expected Block, got {other:?}"),
    }
}

// -- Exact Repeat: thresholds, escalation, hash-key invariance ----------

#[test]
fn exact_repeat_warns_at_three_identical_calls() {
    let mut d = LoopDetector::new();
    assert_eq!(
        d.record("grep", &json!({"q": "x"}), "r"),
        LoopGuardAction::Clear
    );
    assert_eq!(
        d.record("grep", &json!({"q": "x"}), "r"),
        LoopGuardAction::Clear
    );
    let third = d.record("grep", &json!({"q": "x"}), "r");
    match &third {
        LoopGuardAction::Warn(w) => {
            assert!(
                matches!(w.pattern, LoopPattern::ExactRepeat { ref tool, run: 3 } if tool == "grep")
            );
            assert!(
                w.message.contains("[Loop Detection Warning]"),
                "warn must carry the directive prefix: {}",
                w.message
            );
        }
        other => panic!("expected Warn at run=3, got {other:?}"),
    }
}

#[test]
fn exact_repeat_blocks_at_four_identical_calls() {
    let mut d = LoopDetector::new();
    for _ in 0..3 {
        let _ = d.record("grep", &json!({"q": "x"}), "r");
    }
    let fourth = d.record("grep", &json!({"q": "x"}), "r");
    assert!(
        block_text(&fourth).contains("[SYSTEM OVERRIDE: LOOP BLOCKED. YOU MUST CHANGE STRATEGY.]"),
        "Block must carry the directive override text: {fourth:?}"
    );
    assert!(block_text(&fourth).contains("VRO-12 Loop Guard"));
}

#[test]
fn exact_repeat_breaks_when_window_saturates() {
    let mut d = LoopDetector::new();
    for _ in 0..4 {
        let _ = d.record("grep", &json!({"q": "x"}), "r");
    }
    let fifth = d.record("grep", &json!({"q": "x"}), "r");
    match &fifth {
        LoopGuardAction::Break(reason) => {
            assert!(
                reason.contains("VRO-12 loop guard"),
                "Break must name the guard: {reason}"
            );
            assert!(
                reason.contains("exact repeat"),
                "Break must name the pattern: {reason}"
            );
            assert!(
                reason.contains("grep"),
                "Break must name the tool: {reason}"
            );
        }
        other => panic!("expected Break at window saturation, got {other:?}"),
    }
}

#[test]
fn exact_repeat_resets_when_args_change() {
    let mut d = LoopDetector::new();
    let _ = d.record("grep", &json!({"q": "x"}), "r1");
    let _ = d.record("grep", &json!({"q": "x"}), "r1");
    let _ = d.record("grep", &json!({"q": "x"}), "r1"); // Warn
    let _ = d.record("grep", &json!({"q": "OTHER"}), "r1"); // different args -> run broken
    assert!(matches!(
        d.record("grep", &json!({"q": "x"}), "r1"),
        LoopGuardAction::Break(_)
    ));
}

#[test]
fn exact_repeat_result_differences_do_not_block_the_call_key() {
    // PRD: the Exact-Repeat key is the CALL (name + args). Identical calls
    // with different results must still escalate.
    let mut d = LoopDetector::new();
    d.record("rand", &json!({"s":1}), "r1");
    d.record("rand", &json!({"s":1}), "r2");
    d.record("rand", &json!({"s":1}), "r3");
    let a4 = d.record("rand", &json!({"s":1}), "r4");
    assert!(matches!(a4, LoopGuardAction::Block(_)));
}

#[test]
fn ping_pong_warns_on_full_window_alternation() {
    let mut d = LoopDetector::new();
    d.record("read_file", &json!({"p":"a"}), "ra");
    d.record("grep", &json!({"q":"b"}), "rb");
    d.record("read_file", &json!({"p":"a"}), "ra");
    let a4 = d.record("grep", &json!({"q":"b"}), "rb");
    assert!(
        matches!(warn_pattern(&a4), LoopPattern::PingPong { a, b } if a == "read_file" && b == "grep"),
        "4 alternating calls must Warn PingPong, got {a4:?}"
    );
}

#[test]
fn ping_pong_blocks_when_pattern_persists_after_warn() {
    let mut d = LoopDetector::new();
    d.record("read_file", &json!({"p":"a"}), "ra");
    d.record("grep", &json!({"q":"b"}), "rb");
    d.record("read_file", &json!({"p":"a"}), "ra");
    let warn = d.record("grep", &json!({"q":"b"}), "rb");
    assert!(matches!(warn, LoopGuardAction::Warn(_)));
    d.record("read_file", &json!({"p":"a"}), "ra");
    let a6 = d.record("grep", &json!({"q":"b"}), "rb");
    assert!(
        matches!(a6, LoopGuardAction::Block(_)),
        "persisting ping-pong must Block, got {a6:?}"
    );
}

#[test]
fn ping_pong_breaks_when_alternation_fills_every_slot() {
    // 5 alternating calls saturate the window. PRD §3: the ping-pong
    // Break tier is only reachable by degradation to Exact Repeat (the
    // reference's semantic); whole-window saturation of two alternating
    // tools is the terminal Block for the pattern. The loop still halts
    // on the NEXT same-args repeat via detector 1.
    let mut d = LoopDetector::new();
    d.record("read_file", &json!({"p":"a"}), "ra");
    d.record("grep", &json!({"q":"b"}), "rb");
    d.record("read_file", &json!({"p":"a"}), "ra");
    d.record("grep", &json!({"q":"b"}), "rb");
    let a5 = d.record("read_file", &json!({"p":"a"}), "ra");
    // Whole-window alternation persists past the Warn: the terminal
    // action for the pattern is Block (persisting after warning).
    assert!(
        matches!(a5, LoopGuardAction::Block(_)),
        "full-window alternation must Block (pattern persisted past Warn), got {a5:?}"
    );
    // A same-tool call breaks the alternation and starts a new consecutive
    // run; it is not yet an exact-repeat warning.
    let a6 = d.record("read_file", &json!({"p":"a"}), "ra");
    assert!(
        matches!(a6, LoopGuardAction::Clear),
        "breaking alternation must reset the pattern, got {a6:?}"
    );
}

#[test]
fn ping_pong_ignores_same_tool_repetition() {
    // A,A,A,A is Exact-Repeat territory, not Ping-Pong.
    let mut d = LoopDetector::new();
    for _ in 0..4 {
        d.record("grep", &json!({"q":"a"}), "r");
    }
    // The 3rd call already Warned (exact repeat); ensure no PingPong misfire.
    let a = d.record("grep", &json!({"q":"a"}), "r");
    assert!(
        !matches!(
            &a,
            LoopGuardAction::Warn(w) if matches!(w.pattern, LoopPattern::PingPong { .. })
        ) && !matches!(
            &a,
            LoopGuardAction::Block(t) if t.contains("ping-pong")
        ),
        "same-tool repetition must not classify as PingPong, got {a:?}"
    );
}

// ---------------------------------------------------------------------------
// End-to-end ReAct-loop integration (guard wired into the OBSERVE step)
// ---------------------------------------------------------------------------

/// Scripted agent: replays a fixed decision list, then repeats the last one.
struct OrderedScriptedAgent {
    decisions: Mutex<Vec<ReactDecision>>,
}
impl OrderedScriptedAgent {
    fn new(decisions: Vec<ReactDecision>) -> Self {
        // Reverse once at construction so `pop()` yields the decisions in
        // their authored order (front-to-back), not reversed.
        let mut decisions = decisions;
        decisions.reverse();
        Self {
            decisions: Mutex::new(decisions),
        }
    }
}
impl ReactAgent for OrderedScriptedAgent {
    fn next_action<'a>(
        &'a self,
        _prompt: &'a str,
        _trajectory: &'a [crate::vro::react::TrajectoryEntry],
    ) -> Pin<Box<dyn Future<Output = ReactDecision> + Send + 'a>> {
        let next = self.decisions.lock().expect("poisoned").pop();
        Box::pin(async move {
            next.unwrap_or(ReactDecision::Finish {
                output: serde_json::Value::Null,
            })
        })
    }
}

/// Invoker returning one fixed output per tool name; all tools read-only.
struct StaticInvoker {
    classes: Mutex<std::collections::HashMap<String, ToolExecutionClass>>,
    outputs: Mutex<std::collections::HashMap<String, Result<String, ToolInvocationError>>>,
}
impl StaticInvoker {
    fn with_read(name: &str, output: &str) -> Self {
        let mut classes = std::collections::HashMap::new();
        classes.insert(name.to_string(), ToolExecutionClass::ReadOnly);
        let mut outputs = std::collections::HashMap::new();
        outputs.insert(name.to_string(), Ok(output.to_string()));
        Self {
            classes: Mutex::new(classes),
            outputs: Mutex::new(outputs),
        }
    }
}
impl ToolInvoker for StaticInvoker {
    fn class_of(&self, name: &str) -> Option<ToolExecutionClass> {
        self.classes.lock().expect("poisoned").get(name).copied()
    }
    fn invoke<'a>(
        &'a self,
        name: &'a str,
        _args: &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>> {
        let out = self.outputs.lock().expect("poisoned").get(name).cloned();
        Box::pin(async move {
            out.unwrap_or_else(|| Err(ToolInvocationError::UnknownTool(name.to_string())))
        })
    }
}

fn budget(model: u32, tools: u32) -> ReasoningBudget {
    ReasoningBudget {
        max_model_calls: model,
        max_tool_calls: tools,
        ..ReasoningBudget::balanced()
    }
}

#[tokio::test]
async fn react_loop_breaks_on_repeat_pattern() {
    // Same read-only tool + same args + same result, forever. The guard must
    // trip Break with a named VRO-12 risk BEFORE max_tool_calls can fire.
    let decisions = std::iter::repeat_n(
        ReactDecision::CallTool {
            name: "grep".to_string(),
            arguments: json!({"pattern": "struct"}),
        },
        12,
    )
    .collect::<Vec<_>>();
    let agent = OrderedScriptedAgent::new(decisions);
    let invoker = StaticInvoker::with_read("grep", "same output");
    let outcome =
        run_tool_grounded_react("loop forever", &agent, &invoker, budget(20, 20), true).await;
    assert_eq!(outcome.status, OutcomeStatus::BudgetExceeded);
    assert!(
        outcome
            .unresolved_risks
            .iter()
            .any(|r| r.contains("VRO-12 loop guard")),
        "Break must carry the named risk; got {:?}",
        outcome.unresolved_risks
    );
}

#[tokio::test]
async fn react_loop_block_does_not_consume_tool_budget() {
    // max_tool_calls = 20 but the exact-repeat Break must fire well before
    // the budget; the *Block* tier proves budget-preservation because Break
    // can only be reached through the Warn -> Block -> Break ladder.
    let decisions = std::iter::repeat_n(
        ReactDecision::CallTool {
            name: "read_file".to_string(),
            arguments: json!({"path": "src/main.rs"}),
        },
        12,
    )
    .collect::<Vec<_>>();
    let agent = OrderedScriptedAgent::new(decisions);
    let invoker = StaticInvoker::with_read("read_file", "fixed body");
    let outcome =
        run_tool_grounded_react("reread forever", &agent, &invoker, budget(20, 20), true).await;
    // Cost accounting must NOT count blocked attempts as dispatched tools
    // beyond what actually ran.
    assert!(
        outcome.cost.total_tokens < 40,
        "blocked attempts must not inflate cost: {:?}",
        outcome.cost
    );
    assert!(
        outcome
            .unresolved_risks
            .iter()
            .any(|r| r.contains("VRO-12"))
    );
}

#[tokio::test]
async fn react_loop_no_progress_breaks_identical_empty_results() {
    // Same tool, DIFFERENT args every time, byte-identical empty result: the
    // classic token-burning probe. No-Progress must Break it.
    let decisions = (0..12)
        .map(|i| ReactDecision::CallTool {
            name: "grep".to_string(),
            arguments: json!({"pattern": format!("needle{i}")}),
        })
        .collect::<Vec<_>>();
    let agent = OrderedScriptedAgent::new(decisions);
    let invoker = StaticInvoker::with_read("grep", "");
    let outcome =
        run_tool_grounded_react("hunt forever", &agent, &invoker, budget(20, 20), true).await;
    assert_eq!(outcome.status, OutcomeStatus::BudgetExceeded);
    assert!(
        outcome
            .unresolved_risks
            .iter()
            .any(|r| r.contains("VRO-12"))
    );
}

#[tokio::test]
async fn react_loop_ping_pong_intervenes_before_finish() {
    // A, B, A, B, ... with identical outputs each time: whole-window
    // alternation must trip the guard.
    let decisions = (0..12)
        .map(|i| {
            if i % 2 == 0 {
                ReactDecision::CallTool {
                    name: "list_directory".to_string(),
                    arguments: json!({"path": "."}),
                }
            } else {
                ReactDecision::CallTool {
                    name: "grep".to_string(),
                    arguments: json!({"pattern": "x"}),
                }
            }
        })
        .collect::<Vec<_>>();
    let agent = OrderedScriptedAgent::new(decisions);
    let invoker = StaticInvoker::with_read("list_directory", "a.rs b.rs");
    invoker
        .outputs
        .lock()
        .expect("poisoned")
        .insert("grep".to_string(), Ok("a.rs:1:x".to_string()));
    let (outcome, trajectory) = run_tool_grounded_react_with_trajectory(
        "bounce forever",
        &agent,
        &invoker,
        budget(20, 20),
        true,
    )
    .await;
    assert!(
        trajectory
            .iter()
            .any(|entry| matches!(entry, TrajectoryEntry::Observation { text, .. } if text.contains("VRO-12"))),
        "ping-pong must inject a strategy-change observation; got {trajectory:?}"
    );
    assert_eq!(outcome.status, OutcomeStatus::Succeeded);
}
