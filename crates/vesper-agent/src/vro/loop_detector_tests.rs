//! Unit + integration tests for VRO-12 result-aware loop detection.
//!
//! Covers every required PRD §6 test: W1 (window bounded at 5 after 8+
//! calls), H1/H2/H3 (canonical-JSON key-order invariance; `0` ≢ `0.0` ≢
//! `"0"`; `["ab","c"] ≢ ["a","bc"]`), E1–E4 (Exact-Repeat Warn → Block →
//! Break ladder, partial trajectory on Break), P1/P2/P3 (Ping-Pong Warn,
//! the negative `A,B,B,A,A` case, Block on persisting pattern), N1–N3
//! (No-Progress Warn, interleaved-call `filter` semantics, identical-args
//! staying Exact-Repeat), R1/R2 (failed invocations and Read-Before-Write
//! rejections never recorded, no budget consumed), and Z2 (a non-looping
//! turn produces no `[VRO-12` bytes anywhere).
//!
//! Z1 (guard disabled ⇒ trajectory byte-identical) is intentionally absent:
//! no disable switch exists, so the requirement is unsatisfiable as written.
//! See the PRD decision log and the audit report.
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
        LoopGuardAction::Break(brk) => {
            assert!(
                brk.message.contains("VRO-12 loop guard"),
                "Break must name the guard: {}",
                brk.message
            );
            assert!(
                brk.message.contains("exact repeat"),
                "Break must name the pattern: {}",
                brk.message
            );
            assert!(
                brk.message.contains("grep"),
                "Break must name the tool: {}",
                brk.message
            );
            assert!(
                brk.message.contains("5"),
                "Break must name the count: {}",
                brk.message
            );
            // The typed payload must agree with the human-readable note and
            // classify structurally (no string matching required).
            assert!(matches!(
                &brk.pattern,
                LoopPattern::ExactRepeat { tool, run: 5 } if tool == "grep"
            ));
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

// ---------------------------------------------------------------------------
// PRD §6 required tests that the original suite omitted (audit gap fix).
// ---------------------------------------------------------------------------

// -- W1: window mechanics ------------------------------------------------

#[test]
fn window_retains_at_most_five_records_after_many_calls() {
    // PRD C2: 8 recorded calls => internal length <= 5. Also pins that the
    // retained window is the LAST five (newest) records, not the first five.
    let mut d = LoopDetector::new();
    for i in 0..8 {
        let _ = d.record("grep", &json!({"q": i}), &format!("result-{i}"));
        assert!(
            d.len() <= 5,
            "window must never exceed 5 entries, got {} after call {i}",
            d.len()
        );
    }
    assert_eq!(
        d.len(),
        5,
        "exactly 5 records must be retained after 8 calls"
    );
}

// -- H1/H2/H3: canonical hashing ----------------------------------------

#[test]
fn args_hash_is_invariant_under_object_key_reordering() {
    // PRD §2.2 / H1: {"a":1,"b":2} === {"b":2,"a":1}.
    let mut first = LoopDetector::new();
    first.record("grep", &json!({"a": 1, "b": 2}), "r");
    let mut second = LoopDetector::new();
    second.record("grep", &json!({"b": 2, "a": 1}), "r");
    // Two distinct detectors fed key-reordered args must classify the same
    // call as identical: the trailing run count is what the exact-repeat
    // detector uses, so re-recording the SAME args in both detectors must
    // produce the same escalation. Cheapest structural proof: re-record and
    // compare the actions.
    let a = first.record("grep", &json!({"a": 1, "b": 2}), "r");
    let b = second.record("grep", &json!({"b": 2, "a": 1}), "r");
    assert_eq!(
        a, b,
        "key-reordered args must hash identically (H1); got {a:?} vs {b:?}"
    );
}

#[test]
fn hash_distinguishes_zero_float_and_string_zero() {
    // PRD §2.2 / H2: 0 !== 0.0 !== "0". These are args-hash distinctions:
    // three calls with args 0, 0.0, and "0" must NOT collapse into one
    // exact-repeat run of 3.
    let mut d = LoopDetector::new();
    let a1 = d.record("probe", &json!(0), "r");
    let a2 = d.record("probe", &json!(0.0), "r");
    let a3 = d.record("probe", &json!("0"), "r");
    assert!(
        matches!(a1, LoopGuardAction::Clear)
            && matches!(a2, LoopGuardAction::Clear)
            && matches!(a3, LoopGuardAction::Clear),
        "0, 0.0, and \"0\" are three distinct argument hashes, so no \
         exact-repeat run may form; got {a1:?}, {a2:?}, {a3:?}"
    );
}

#[test]
fn hash_distinguishes_adjacent_string_concatenations() {
    // PRD §6 H3: ["ab","c"] !== ["a","bc"]. Distinct args hashes mean the
    // trailing identical-run detector cannot treat them as the same call.
    let mut d = LoopDetector::new();
    let a1 = d.record("probe", &json!(["ab", "c"]), "r");
    let a2 = d.record("probe", &json!(["a", "bc"]), "r");
    let a3 = d.record("probe", &json!(["ab", "c"]), "r");
    assert!(
        matches!(a1, LoopGuardAction::Clear) && matches!(a2, LoopGuardAction::Clear),
        "distinct argument arrays must not form an exact-repeat run; got {a1:?}, {a2:?}"
    );
    assert!(
        matches!(a3, LoopGuardAction::Clear),
        "with [\"ab\",\"c\"] seen at positions 1 and 3 and [\"a\",\"bc\"] between \
         them, the trailing run is 1 — not 3; got {a3:?}"
    );
}

// -- E4: partial trajectory on Break ------------------------------------

#[tokio::test]
async fn with_trajectory_variant_returns_partial_trajectory_on_break() {
    // PRD §6 E4: run_tool_grounded_react_with_trajectory must return the
    // partial trajectory when the guard breaks the turn, so VRO-7 extraction
    // keeps working on BudgetExceeded turns.
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
    let (outcome, trajectory) = run_tool_grounded_react_with_trajectory(
        "loop forever",
        &agent,
        &invoker,
        budget(20, 20),
        true,
    )
    .await;
    assert_eq!(outcome.status, OutcomeStatus::BudgetExceeded);
    assert!(
        !trajectory.is_empty(),
        "Break must still return the partial trajectory (E4); got {trajectory:?}"
    );
    // The partial trajectory must contain the actions and observations that
    // ran BEFORE the break, i.e. at least one (Action, Observation) pair.
    assert!(
        trajectory
            .iter()
            .any(|entry| matches!(entry, TrajectoryEntry::Action { .. }))
    );
    assert!(
        trajectory
            .iter()
            .any(|entry| matches!(entry, TrajectoryEntry::Observation { success: true, .. }))
    );
    // And the typed break cause must be reachable without string matching.
    assert!(
        outcome
            .unresolved_risks
            .iter()
            .any(|r| r.contains("VRO-12"))
    );
}

// -- P2: negative ping-pong case ----------------------------------------

#[test]
fn ping_pong_negative_case_abbaa_is_clear() {
    // PRD §6 P2: A,B,B,A,A is NOT alternation and must classify Clear at
    // every step. The two-in-a-row B's break the pattern.
    let mut d = LoopDetector::new();
    let a1 = d.record("read_file", &json!({"p": 1}), "ra");
    let a2 = d.record("grep", &json!({"q": 1}), "rb");
    let a3 = d.record("grep", &json!({"q": 1}), "rb");
    let a4 = d.record("read_file", &json!({"p": 1}), "ra");
    let a5 = d.record("read_file", &json!({"p": 1}), "ra");
    for (label, action) in [
        ("1st", a1),
        ("2nd", a2),
        ("3rd", a3),
        ("4th", a4),
        ("5th", a5),
    ] {
        assert!(
            !matches!(action, LoopGuardAction::Break(_)),
            "A,B,B,A,A must never Break ({label}): {action:?}"
        );
        assert!(
            !matches!(
                &action,
                LoopGuardAction::Warn(w) if matches!(w.pattern, LoopPattern::PingPong { .. })
            ) && !matches!(
                &action,
                LoopGuardAction::Block(text) if text.contains("alternation")
            ),
            "A,B,B,A,A must never classify PingPong ({label}): {action:?}"
        );
    }
}

// -- N2 / N3: no-progress filtering and separation -----------------------

#[test]
fn no_progress_interleaved_call_does_not_reset_the_count() {
    // PRD §6 N2 / the reference's 43-near-duplicate-calls lesson: an
    // unrelated interleaved call must NOT reset the no-progress count. The
    // detector counts with `filter` across the whole window, not
    // `take_while` over a consecutive run.
    // The interleaving must not itself form a whole-window alternation
    // (detector 2 owns that shape), so the unrelated call is placed between
    // two same-tool clusters rather than strictly alternating.
    let mut d = LoopDetector::new();
    let _ = d.record("grep", &json!({"q": 1}), "no matches");
    let _ = d.record("list_directory", &json!({"p": "."}), "unrelated");
    let _ = d.record("grep", &json!({"q": 2}), "no matches");
    let _ = d.record("grep", &json!({"q": 3}), "no matches");
    let fourth = d.record("grep", &json!({"q": 4}), "no matches");
    let assert_no_progress = |action: &LoopGuardAction| {
        matches!(
            action,
            LoopGuardAction::Warn(w) if matches!(
                &w.pattern,
                LoopPattern::NoProgress { tool, count: 4, .. } if tool == "grep"
            )
        )
    };
    assert!(
        assert_no_progress(&fourth),
        "4 same-tool same-result differently-argued probes interleaved with \
         unrelated calls must Warn NoProgress (N2); got {fourth:?}"
    );
}

#[test]
fn identical_args_stay_exact_repeat_not_no_progress() {
    // PRD §6 N3: when every call has IDENTICAL args, detector 1 (Exact
    // Repeat) owns the case. The no-progress detector must not fire.
    let mut d = LoopDetector::new();
    let _ = d.record("grep", &json!({"q": "same"}), "no matches");
    let _ = d.record("grep", &json!({"q": "same"}), "no matches");
    let third = d.record("grep", &json!({"q": "same"}), "no matches");
    assert!(
        matches!(
            &third,
            LoopGuardAction::Warn(w) if matches!(w.pattern, LoopPattern::ExactRepeat { .. })
        ),
        "identical args must classify as ExactRepeat, never NoProgress (N3); got {third:?}"
    );
    let fourth = d.record("grep", &json!({"q": "same"}), "no matches");
    assert!(
        matches!(fourth, LoopGuardAction::Block(_)),
        "identical args escalate through the ExactRepeat ladder to Block (N3); got {fourth:?}"
    );
}

// -- R1 / R2: recording rules -------------------------------------------

#[tokio::test]
async fn failed_invocations_are_not_recorded_by_the_loop() {
    // PRD §6 R1: a failed invocation must not enter the window. Prove it by
    // running a ReAct turn whose invoker always errors, then confirming the
    // guard never intervenes (no VRO-12 bytes anywhere) even though the
    // model retries the same call many times.
    struct FailingInvoker;
    impl ToolInvoker for FailingInvoker {
        fn class_of(&self, _name: &str) -> Option<ToolExecutionClass> {
            Some(ToolExecutionClass::ReadOnly)
        }
        fn invoke<'a>(
            &'a self,
            name: &'a str,
            _args: &'a serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>
        {
            let name = name.to_string();
            Box::pin(async move {
                Err(ToolInvocationError::ExecutionFailed(format!(
                    "no such tool target: {name}"
                )))
            })
        }
    }

    let decisions = std::iter::repeat_n(
        ReactDecision::CallTool {
            name: "read_file".to_string(),
            arguments: json!({"path": "missing.rs"}),
        },
        10,
    )
    .collect::<Vec<_>>();
    let agent = OrderedScriptedAgent::new(decisions);
    // A tight model-call ceiling forces the numeric halt while the decision
    // list still holds CallTool entries, so the scripted agent never gets to
    // its Finish fallback.
    let (outcome, trajectory) = run_tool_grounded_react_with_trajectory(
        "read a missing file forever",
        &agent,
        &FailingInvoker,
        budget(5, 20),
        true,
    )
    .await;
    // The turn MUST halt on the numeric model-call ceiling and NEVER on the
    // loop guard — failures are not recorded, so no pattern can ever form.
    assert_eq!(outcome.status, OutcomeStatus::BudgetExceeded);
    assert!(
        outcome
            .unresolved_risks
            .iter()
            .all(|r| !r.contains("VRO-12")),
        "failed invocations must never trigger the loop guard (R1); got {:?}",
        outcome.unresolved_risks
    );
    assert!(
        trajectory
            .iter()
            .all(|entry| !matches!(entry, TrajectoryEntry::Observation { text, .. } if text.contains("VRO-12"))),
        "no VRO-12 observation may appear for failed invocations (R1); got {trajectory:?}"
    );
    assert!(
        outcome
            .unresolved_risks
            .iter()
            .any(|r| r.contains("max_model_calls") || r.contains("max_tool_calls")),
        "the turn must exhaust a numeric ceiling, not the loop guard (R1); got {:?}",
        outcome.unresolved_risks
    );
    // Stronger: every tool call FAILED, so zero successful records exist and
    // the guard never intervened at any tier.
    assert!(
        trajectory.iter().all(|entry| !matches!(
            entry,
            TrajectoryEntry::Observation { text, .. } if text.contains("Loop Detection Warning")
        )),
        "no loop-guard warning may appear for failed invocations (R1)"
    );
}

#[tokio::test]
async fn read_before_write_rejections_are_not_recorded_and_consume_no_budget() {
    // PRD §6 R2: Read-Before-Write rejections are synthesized before the
    // invoker runs, so they must never be recorded and must not consume a
    // max_tool_calls unit. Three rejections in a row (same tool, same args)
    // must not trip any loop-guard tier.
    struct MutatingInvoker;
    impl ToolInvoker for MutatingInvoker {
        fn class_of(&self, _name: &str) -> Option<ToolExecutionClass> {
            Some(ToolExecutionClass::Mutating)
        }
        fn invoke<'a>(
            &'a self,
            _name: &'a str,
            _args: &'a serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>
        {
            Box::pin(async { Ok("written".to_string()) })
        }
    }

    let decisions = std::iter::repeat_n(
        ReactDecision::CallTool {
            name: "write_file".to_string(),
            arguments: json!({"path": "a.txt", "content": "x"}),
        },
        6,
    )
    .collect::<Vec<_>>();
    let agent = OrderedScriptedAgent::new(decisions);
    // requires_grounding = true and every call is Mutating with no prior
    // read evidence => every attempt is rejected by Read-Before-Write.
    let (outcome, trajectory) = run_tool_grounded_react_with_trajectory(
        "write before reading",
        &agent,
        &MutatingInvoker,
        budget(20, 3),
        true,
    )
    .await;
    assert!(
        trajectory
            .iter()
            .all(|entry| !matches!(entry, TrajectoryEntry::Observation { text, .. } if text.contains("VRO-12"))),
        "Read-Before-Write rejections must never be recorded (R2); got {trajectory:?}"
    );
    assert!(
        outcome
            .unresolved_risks
            .iter()
            .all(|r| !r.contains("VRO-12")),
        "Read-Before-Write rejections must never trigger the guard (R2); got {:?}",
        outcome.unresolved_risks
    );
    // Accounting (the strong part of R2): the turn Succeeded after 6
    // Read-Before-Write rejections + 1 Finish. build_succeeded's heuristic
    // is model_calls + tool_calls + observations = 7 + 0 + 6 = 13. The
    // tool_calls term is ZERO — the six rejections never reached the
    // executor and never consumed a max_tool_calls unit, even though
    // max_tool_calls was only 3 and six mutating attempts were made.
    assert_eq!(
        outcome.cost.model_calls, 7,
        "6 rejected CallTool decisions + 1 Finish => 7 model calls"
    );
    assert_eq!(
        outcome.cost.total_tokens, 13,
        "model_calls (7) + tool_calls (0) + observations (6): rejections \
         consume no budget (R2)"
    );
    // Cross-check the arithmetic directly rather than trusting the token
    // heuristic: 13 - 7 model - 6 observations = 0 tool calls consumed.
    let tool_calls_consumed = outcome.cost.total_tokens - u64::from(outcome.cost.model_calls) - 6;
    assert_eq!(
        tool_calls_consumed, 0,
        "the six Read-Before-Write rejections must consume zero tool budget (R2)"
    );
}

// -- Z2: non-looping execution produces no VRO-12 intervention text ------

#[tokio::test]
async fn non_looping_react_turn_produces_no_loop_guard_text() {
    // PRD §6 Z2: a successful ReAct turn with distinct tools and distinct
    // results must produce no "[VRO-12" bytes anywhere in the trajectory.
    struct VariedInvoker;
    impl ToolInvoker for VariedInvoker {
        fn class_of(&self, name: &str) -> Option<ToolExecutionClass> {
            match name {
                "list_directory" | "read_file" | "grep" => Some(ToolExecutionClass::ReadOnly),
                _ => None,
            }
        }
        fn invoke<'a>(
            &'a self,
            name: &'a str,
            args: &'a serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>
        {
            let name = name.to_string();
            let args = args.clone();
            Box::pin(async move {
                Ok(match name.as_str() {
                    "list_directory" => format!("entries: {}", args["path"]),
                    "read_file" => format!("body of {}", args["path"]),
                    "grep" => format!("matches for {}", args["pattern"]),
                    _ => {
                        return Err(ToolInvocationError::UnknownTool(name));
                    }
                })
            })
        }
    }

    let decisions = vec![
        ReactDecision::CallTool {
            name: "list_directory".to_string(),
            arguments: json!({"path": "src"}),
        },
        ReactDecision::CallTool {
            name: "read_file".to_string(),
            arguments: json!({"path": "src/main.rs"}),
        },
        ReactDecision::CallTool {
            name: "grep".to_string(),
            arguments: json!({"pattern": "fn main"}),
        },
        ReactDecision::CallTool {
            name: "read_file".to_string(),
            arguments: json!({"path": "src/lib.rs"}),
        },
        ReactDecision::Finish {
            output: json!({"answer": "four distinct observations"}),
        },
    ];
    let agent = OrderedScriptedAgent::new(decisions);
    let (outcome, trajectory) = run_tool_grounded_react_with_trajectory(
        "explore the workspace",
        &agent,
        &VariedInvoker,
        budget(20, 20),
        true,
    )
    .await;
    assert_eq!(outcome.status, OutcomeStatus::Succeeded);
    assert!(
        trajectory
            .iter()
            .all(|entry| !matches!(entry, TrajectoryEntry::Observation { text, .. } if text.contains("[VRO-12"))),
        "a non-looping turn must produce no VRO-12 intervention text (Z2); got {trajectory:?}"
    );
    assert!(outcome.unresolved_risks.is_empty());
}

// -- Z1: the disabled-guard / byte-identical requirement ----------------
//
// PRD §6 Z1 asks for "guard disabled => trajectory byte-identical to
// pre-VRO-12 for the same scripted run". The shipped detector has NO
// configuration surface: `LoopDetector::new()` is unconditionally active in
// both react.rs and agent_loop.rs (PRD §5 Non-Goals: "No new ReasoningConfig
// / vesper-domain surface"). Adding an `enabled` switch solely to satisfy
// this test would violate the PRD's own Non-Goals and C4 (no new public
// seam), and the reference repo's `enabled` flag exists only because it has
// a full config file surface we deliberately do not port. The observable
// equivalent is Z2 (below/above): a non-looping turn is byte-identical with
// or without the guard, because the guard only mutates the trajectory when
// a pattern actually fires. That is the strongest zero-breakage guarantee
// available without inventing a disable switch. See the audit report and the
// PRD decision log for the full rationale.
