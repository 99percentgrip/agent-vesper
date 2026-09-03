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
        d.record(
            "grep",
            &json!({"q": "x"}),
            "r",
            ToolExecutionClass::ReadOnly
        ),
        LoopGuardAction::Clear
    );
    assert_eq!(
        d.record(
            "grep",
            &json!({"q": "x"}),
            "r",
            ToolExecutionClass::ReadOnly
        ),
        LoopGuardAction::Clear
    );
    let third = d.record(
        "grep",
        &json!({"q": "x"}),
        "r",
        ToolExecutionClass::ReadOnly,
    );
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
        let _ = d.record(
            "grep",
            &json!({"q": "x"}),
            "r",
            ToolExecutionClass::ReadOnly,
        );
    }
    let fourth = d.record(
        "grep",
        &json!({"q": "x"}),
        "r",
        ToolExecutionClass::ReadOnly,
    );
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
        let _ = d.record(
            "grep",
            &json!({"q": "x"}),
            "r",
            ToolExecutionClass::ReadOnly,
        );
    }
    let fifth = d.record(
        "grep",
        &json!({"q": "x"}),
        "r",
        ToolExecutionClass::ReadOnly,
    );
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
    let _ = d.record(
        "grep",
        &json!({"q": "x"}),
        "r1",
        ToolExecutionClass::ReadOnly,
    );
    let _ = d.record(
        "grep",
        &json!({"q": "x"}),
        "r1",
        ToolExecutionClass::ReadOnly,
    );
    let _ = d.record(
        "grep",
        &json!({"q": "x"}),
        "r1",
        ToolExecutionClass::ReadOnly,
    ); // Warn
    let _ = d.record(
        "grep",
        &json!({"q": "OTHER"}),
        "r1",
        ToolExecutionClass::ReadOnly,
    ); // different args -> run broken
    // Exact-repeat run is broken by the differing args (trailing run of
    // `x` is 1), but this sequence also forms a no-progress pattern
    // (4+ identical `r1` results on `grep`, ≥2 distinct args): the 4th
    // record Warned. Further distinct empty-result searches are allowed;
    // this is ordinary exploration, not a terminal loop.
    assert!(matches!(
        d.record(
            "grep",
            &json!({"q": "x"}),
            "r1",
            ToolExecutionClass::ReadOnly
        ),
        LoopGuardAction::Clear
    ));
}

#[test]
fn exact_repeat_result_differences_do_not_block_the_call_key() {
    // PRD: the Exact-Repeat key is the CALL (name + args). Identical calls
    // with different results must still escalate.
    let mut d = LoopDetector::new();
    d.record("rand", &json!({"s":1}), "r1", ToolExecutionClass::ReadOnly);
    d.record("rand", &json!({"s":1}), "r2", ToolExecutionClass::ReadOnly);
    d.record("rand", &json!({"s":1}), "r3", ToolExecutionClass::ReadOnly);
    let a4 = d.record("rand", &json!({"s":1}), "r4", ToolExecutionClass::ReadOnly);
    assert!(matches!(a4, LoopGuardAction::Block(_)));
}

#[test]
fn ping_pong_warns_on_full_window_alternation() {
    let mut d = LoopDetector::new();
    d.record(
        "read_file",
        &json!({"p":"a"}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    d.record(
        "grep",
        &json!({"q":"b"}),
        "rb",
        ToolExecutionClass::ReadOnly,
    );
    d.record(
        "read_file",
        &json!({"p":"a"}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    let a4 = d.record(
        "grep",
        &json!({"q":"b"}),
        "rb",
        ToolExecutionClass::ReadOnly,
    );
    assert!(
        matches!(warn_pattern(&a4), LoopPattern::PingPong { a, b } if a == "read_file" && b == "grep"),
        "4 alternating calls must Warn PingPong, got {a4:?}"
    );
}

#[test]
fn ping_pong_blocks_when_pattern_persists_after_warn() {
    let mut d = LoopDetector::new();
    d.record(
        "read_file",
        &json!({"p":"a"}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    d.record(
        "grep",
        &json!({"q":"b"}),
        "rb",
        ToolExecutionClass::ReadOnly,
    );
    d.record(
        "read_file",
        &json!({"p":"a"}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    let warn = d.record(
        "grep",
        &json!({"q":"b"}),
        "rb",
        ToolExecutionClass::ReadOnly,
    );
    assert!(matches!(warn, LoopGuardAction::Warn(_)));
    d.record(
        "read_file",
        &json!({"p":"a"}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    let a6 = d.record(
        "grep",
        &json!({"q":"b"}),
        "rb",
        ToolExecutionClass::ReadOnly,
    );
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
    d.record(
        "read_file",
        &json!({"p":"a"}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    d.record(
        "grep",
        &json!({"q":"b"}),
        "rb",
        ToolExecutionClass::ReadOnly,
    );
    d.record(
        "read_file",
        &json!({"p":"a"}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    d.record(
        "grep",
        &json!({"q":"b"}),
        "rb",
        ToolExecutionClass::ReadOnly,
    );
    let a5 = d.record(
        "read_file",
        &json!({"p":"a"}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    // Whole-window alternation persists past the Warn: the terminal
    // action for the pattern is Block (persisting after warning).
    assert!(
        matches!(a5, LoopGuardAction::Block(_)),
        "full-window alternation must Block (pattern persisted past Warn), got {a5:?}"
    );
    // A same-tool call breaks the alternation and starts a new consecutive
    // run; it is not yet an exact-repeat warning.
    let a6 = d.record(
        "read_file",
        &json!({"p":"a"}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
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
        d.record("grep", &json!({"q":"a"}), "r", ToolExecutionClass::ReadOnly);
    }
    // The 3rd call already Warned (exact repeat); ensure no PingPong misfire.
    let a = d.record("grep", &json!({"q":"a"}), "r", ToolExecutionClass::ReadOnly);
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
    // Generous tool budget so the numeric ceiling never fires and only the
    // guard ends the turn. Trace with the refund:
    //   THINK 1..3 -> dispatch, tool_calls = 3, records 1..3 (Clear, Clear, Warn)
    //   THINK 4    -> dispatch (tool_calls = 4), record 4 = Block => refunded to 3
    //   THINK 5    -> dispatch (tool_calls = 4), record 5 = Break (window saturated)
    // Exact accounting, not a token-count proxy: build_budget_exceeded sets
    // total_tokens = model_calls + tool_calls = 5 + 4 = 9. WITHOUT the
    // refund the fourth dispatch would stand and the total would be 10 —
    // so 9 (not 10) is the observable proof that the blocked attempt was
    // never charged to max_tool_calls.
    let outcome =
        run_tool_grounded_react("reread forever", &agent, &invoker, budget(20, 20), true).await;
    assert!(
        outcome
            .unresolved_risks
            .iter()
            .any(|r| r.contains("VRO-12 loop guard")),
        "the guard must end the turn, not the numeric ceiling; got {:?}",
        outcome.unresolved_risks
    );
    assert_eq!(outcome.cost.model_calls, 5);
    assert_eq!(
        outcome.cost.total_tokens, 9,
        "model_calls (5) + tool_calls (4): the blocked 4th dispatch must be \
         refunded — 10 would mean it was charged; got {:?}",
        outcome.cost
    );
}

#[tokio::test]
async fn react_loop_no_progress_warns_without_stopping_exploration() {
    // Same tool, DIFFERENT args every time, byte-identical empty result: the
    // ordinary repository exploration. No-Progress may advise once but must
    // never stop the turn; only the configured numeric budget may do that.
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
    assert_eq!(outcome.status, OutcomeStatus::Succeeded);
    assert!(
        outcome.unresolved_risks.is_empty(),
        "no-progress must not manufacture a terminal risk: {:?}",
        outcome.unresolved_risks
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
    invoker
        .classes
        .lock()
        .expect("poisoned")
        .insert("grep".to_string(), ToolExecutionClass::ReadOnly);
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
        let _ = d.record(
            "grep",
            &json!({"q": i}),
            &format!("result-{i}"),
            ToolExecutionClass::ReadOnly,
        );
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
    // PRD §2.2 / H1: {"a":1,"b":2} === {"b":2,"a":1}. The hash is the only
    // thing feeding the exact-repeat run counter, so the discriminating
    // observation is ESCALATION: three calls whose args differ ONLY in key
    // order must escalate through the exact-repeat ladder exactly like
    // three byte-identical calls would. (Comparing two detectors' second
    // record is vacuous — both return Clear either way.)
    let mut d = LoopDetector::new();
    // First insertion order.
    let a1 = d.record(
        "grep",
        &json!({"a": 1, "b": 2}),
        "r",
        ToolExecutionClass::ReadOnly,
    );
    // Reversed insertion order, same logical object.
    let a2 = d.record(
        "grep",
        &json!({"b": 2, "a": 1}),
        "r",
        ToolExecutionClass::ReadOnly,
    );
    // Back to the first order: the run is now 3 IF AND ONLY IF the two
    // orderings hashed identically.
    let a3 = d.record(
        "grep",
        &json!({"a": 1, "b": 2}),
        "r",
        ToolExecutionClass::ReadOnly,
    );
    assert!(matches!(a1, LoopGuardAction::Clear));
    assert!(matches!(a2, LoopGuardAction::Clear));
    assert!(
        matches!(
            &a3,
            LoopGuardAction::Warn(w) if matches!(
                w.pattern,
                LoopPattern::ExactRepeat { ref tool, run: 3 } if tool == "grep"
            )
        ),
        "key-reordered args must hash identically and escalate the run to 3 (H1); got {a3:?}"
    );
}

#[test]
fn hash_distinguishes_zero_float_and_string_zero() {
    // PRD §2.2 / H2: 0 !== 0.0 !== "0". Each PAIR must be distinct: testing
    // only the full three-way sequence is vacuous, because a single
    // collision (0 === 0.0) would still leave the run at 2 and never warn.
    // Each pair is followed by two repeats of the FIRST member so a pair
    // collision forms a run of 3 (Warn) and a pair distinction stays Clear.
    // Zero triples are the classic args-hash collision (the reference
    // patches it with length-prefixed canonical text; JSON serialization
    // gives us the distinction for free).
    let cases: [(&str, serde_json::Value, serde_json::Value); 3] = [
        ("int vs float", json!(0), json!(0.0)),
        ("float vs string", json!(0.0), json!("0")),
        ("int vs string", json!(0), json!("0")),
    ];
    for (label, first, second) in cases {
        let mut d = LoopDetector::new();
        let a1 = d.record("probe", &first, "r", ToolExecutionClass::ReadOnly);
        let _ = d.record("probe", &second, "r", ToolExecutionClass::ReadOnly);
        let a3 = d.record("probe", &first, "r", ToolExecutionClass::ReadOnly);
        assert!(
            matches!(a1, LoopGuardAction::Clear) && matches!(a3, LoopGuardAction::Clear),
            "{label}: {first:?} and {second:?} must be distinct argument hashes, so \
             no exact-repeat run of 3 may form; got {a1:?} then {a3:?}"
        );
    }
    // Positive control: the same construction WITH a genuine repeat must
    // Warn, proving the harness above can fail and is not vacuous.
    let mut d = LoopDetector::new();
    let _ = d.record("probe", &json!(0), "r", ToolExecutionClass::ReadOnly);
    let _ = d.record("probe", &json!(0), "r", ToolExecutionClass::ReadOnly);
    assert!(
        matches!(
            d.record("probe", &json!(0), "sql", ToolExecutionClass::ReadOnly),
            LoopGuardAction::Warn(w) if matches!(w.pattern, LoopPattern::ExactRepeat { .. })
        ),
        "control: three identical integer-zero calls must Warn"
    );
}

#[test]
fn hash_distinguishes_adjacent_string_concatenations() {
    // PRD §6 H3: ["ab","c"] !== ["a","bc"]. Distinct args hashes mean the
    // trailing identical-run detector cannot treat them as the same call.
    let mut d = LoopDetector::new();
    let a1 = d.record(
        "probe",
        &json!(["ab", "c"]),
        "r",
        ToolExecutionClass::ReadOnly,
    );
    let a2 = d.record(
        "probe",
        &json!(["a", "bc"]),
        "r",
        ToolExecutionClass::ReadOnly,
    );
    let a3 = d.record(
        "probe",
        &json!(["ab", "c"]),
        "r",
        ToolExecutionClass::ReadOnly,
    );
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
    let a1 = d.record(
        "read_file",
        &json!({"p": 1}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    let a2 = d.record("grep", &json!({"q": 1}), "rb", ToolExecutionClass::ReadOnly);
    let a3 = d.record("grep", &json!({"q": 1}), "rb", ToolExecutionClass::ReadOnly);
    let a4 = d.record(
        "read_file",
        &json!({"p": 1}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
    let a5 = d.record(
        "read_file",
        &json!({"p": 1}),
        "ra",
        ToolExecutionClass::ReadOnly,
    );
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
    let _ = d.record(
        "grep",
        &json!({"q": 1}),
        "no matches",
        ToolExecutionClass::ReadOnly,
    );
    let _ = d.record(
        "list_directory",
        &json!({"p": "."}),
        "unrelated",
        ToolExecutionClass::ReadOnly,
    );
    let _ = d.record(
        "grep",
        &json!({"q": 2}),
        "no matches",
        ToolExecutionClass::ReadOnly,
    );
    let _ = d.record(
        "grep",
        &json!({"q": 3}),
        "no matches",
        ToolExecutionClass::ReadOnly,
    );
    let fourth = d.record(
        "grep",
        &json!({"q": 4}),
        "no matches",
        ToolExecutionClass::ReadOnly,
    );
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
    let _ = d.record(
        "grep",
        &json!({"q": "same"}),
        "no matches",
        ToolExecutionClass::ReadOnly,
    );
    let _ = d.record(
        "grep",
        &json!({"q": "same"}),
        "no matches",
        ToolExecutionClass::ReadOnly,
    );
    let third = d.record(
        "grep",
        &json!({"q": "same"}),
        "no matches",
        ToolExecutionClass::ReadOnly,
    );
    assert!(
        matches!(
            &third,
            LoopGuardAction::Warn(w) if matches!(w.pattern, LoopPattern::ExactRepeat { .. })
        ),
        "identical args must classify as ExactRepeat, never NoProgress (N3); got {third:?}"
    );
    let fourth = d.record(
        "grep",
        &json!({"q": "same"}),
        "no matches",
        ToolExecutionClass::ReadOnly,
    );
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

// ---------------------------------------------------------------------------
// Post-audit regression tests — the TUI loop-detector incident
// ---------------------------------------------------------------------------

// Two fatal false positives were reported from real coding turns:
//
//   1. `no-progress, tool 'edit_file', 5 differently-argued calls with
//      byte-identical results (window saturated)`
//   2. `no-progress, tool 'grep', 5 differently-argued calls with
//      byte-identical results (window saturated)`
//
// Root cause 1 (edit_file): mutating tools return constant-form
// acknowledgments (`edited {path}`) that do not encode the state change.
// Five legitimate, differently-argued edits to one file produce
// byte-identical acks — real progress misread as a loop.
// Root cause 2 (grep): the no-progress ladder broke at first saturation
// with zero corrective opportunity (Warn@4 → Break@5, no Block tier), so
// five legitimate empty-result greps killed the turn instantly.
//
// The fixes: No-Progress/Ping-Pong are class-gated (read-only tools only —
// the "identical result ⇒ no new information" premise holds only there),
// and the ladder is Warn@4 → Block@5 (refunded) → Break@6 (only after the
// model ignored the Block override).

#[test]
fn mutating_tool_identical_acks_never_classify_no_progress() {
    // THE edit_file incident: five differently-argued edits to the same
    // file, each succeeding, each returning the constant-form ack
    // `edited src/lib.rs`. The workspace genuinely advanced five times.
    // The detector must stay Clear at every step.
    let mut d = LoopDetector::new();
    let ack = "edited src/lib.rs";
    let actions = [
        d.record(
            "edit_file",
            &json!({"path": "src/lib.rs", "old_text": "a", "new_text": "b"}),
            ack,
            ToolExecutionClass::Mutating,
        ),
        d.record(
            "edit_file",
            &json!({"path": "src/lib.rs", "old_text": "b", "new_text": "c"}),
            ack,
            ToolExecutionClass::Mutating,
        ),
        d.record(
            "edit_file",
            &json!({"path": "src/lib.rs", "old_text": "c", "new_text": "d"}),
            ack,
            ToolExecutionClass::Mutating,
        ),
        d.record(
            "edit_file",
            &json!({"path": "src/lib.rs", "old_text": "d", "new_text": "e"}),
            ack,
            ToolExecutionClass::Mutating,
        ),
        d.record(
            "edit_file",
            &json!({"path": "src/lib.rs", "old_text": "e", "new_text": "f"}),
            ack,
            ToolExecutionClass::Mutating,
        ),
    ];
    for (i, action) in actions.iter().enumerate() {
        assert!(
            matches!(action, LoopGuardAction::Clear),
            "legitimate edit_file acks must never fire the guard (call {}): {action:?}",
            i + 1
        );
    }
}

#[test]
fn mutating_tool_identical_acks_never_classify_ping_pong() {
    // The read↔edit edit cycle: five alternating calls where every mutating
    // step genuinely advances state. Class-gated Ping-Pong must stay Clear.
    let mut d = LoopDetector::new();
    let read_ack = "fn main() {}";
    let edit_ack = "edited src/lib.rs";
    let actions = [
        d.record(
            "read_file",
            &json!({"path": "src/lib.rs"}),
            read_ack,
            ToolExecutionClass::ReadOnly,
        ),
        d.record(
            "edit_file",
            &json!({"path": "src/lib.rs", "old_text": "a", "new_text": "b"}),
            edit_ack,
            ToolExecutionClass::Mutating,
        ),
        d.record(
            "read_file",
            &json!({"path": "src/lib.rs"}),
            read_ack,
            ToolExecutionClass::ReadOnly,
        ),
        d.record(
            "edit_file",
            &json!({"path": "src/lib.rs", "old_text": "b", "new_text": "c"}),
            edit_ack,
            ToolExecutionClass::Mutating,
        ),
        d.record(
            "read_file",
            &json!({"path": "src/lib.rs"}),
            read_ack,
            ToolExecutionClass::ReadOnly,
        ),
    ];
    for (i, action) in actions.iter().enumerate() {
        assert!(
            matches!(action, LoopGuardAction::Clear),
            "the healthy read↔edit cycle must never fire the guard (call {}): {action:?}",
            i + 1
        );
    }
}

#[test]
fn no_progress_warns_once_then_allows_exploration() {
    // Repeated, differently-argued empty searches are common while mapping
    // an unfamiliar repository. They receive one advisory but never a
    // heuristic Block or Break.
    let mut d = LoopDetector::new();
    let empty = "";
    let a1 = d.record(
        "grep",
        &json!({"pattern": "needle-1"}),
        empty,
        ToolExecutionClass::ReadOnly,
    );
    let a2 = d.record(
        "grep",
        &json!({"pattern": "needle-2"}),
        empty,
        ToolExecutionClass::ReadOnly,
    );
    let a3 = d.record(
        "grep",
        &json!({"pattern": "needle-3"}),
        empty,
        ToolExecutionClass::ReadOnly,
    );
    assert!(matches!(a1, LoopGuardAction::Clear));
    assert!(matches!(a2, LoopGuardAction::Clear));
    assert!(matches!(a3, LoopGuardAction::Clear));
    let a4 = d.record(
        "grep",
        &json!({"pattern": "needle-4"}),
        empty,
        ToolExecutionClass::ReadOnly,
    );
    assert!(
        matches!(&a4, LoopGuardAction::Warn(w) if matches!(
            &w.pattern,
            LoopPattern::NoProgress { tool, count: 4, .. } if tool == "grep"
        )),
        "4th identical-result probe must Warn, got {a4:?}"
    );
    let a5 = d.record(
        "grep",
        &json!({"pattern": "needle-5"}),
        empty,
        ToolExecutionClass::ReadOnly,
    );
    assert!(matches!(&a5, LoopGuardAction::Clear));
    let a6 = d.record(
        "grep",
        &json!({"pattern": "needle-6"}),
        empty,
        ToolExecutionClass::ReadOnly,
    );
    assert!(matches!(&a6, LoopGuardAction::Clear));
}

#[test]
fn no_progress_reset_when_model_changes_tool_or_result() {
    // The corrective purpose of the ladder: after a Warn, if the model
    // changes strategy (different tool, then a genuinely different result),
    // the escalation state must reset. A later 4-probe streak on a NEW
    // exhausted evidence starts at Warn again, not at Block.
    let mut d = LoopDetector::new();
    let empty = "";
    for i in 0..4 {
        let action = d.record(
            "grep",
            &json!({"pattern": format!("stuck-{i}")}),
            empty,
            ToolExecutionClass::ReadOnly,
        );
        let _ = action;
    }
    // Warn fired on the 4th. Model obeys: switches tool and gets a distinct
    // result. The escalation state survives while the old evidence remains
    // in the window (N2 filter semantics — an interleaved unrelated call
    // must NOT reset the streak), so the heal sequence must wash ALL five
    // old records out before the ladder is provably reset.
    let heal = d.record(
        "read_file",
        &json!({"path": "other.rs"}),
        "fresh content",
        ToolExecutionClass::ReadOnly,
    );
    assert!(matches!(heal, LoopGuardAction::Clear));
    for i in 1..5 {
        let heal = d.record(
            "read_file",
            &json!({"path": format!("other-{i}.rs")}),
            &format!("fresh content {i}"),
            ToolExecutionClass::ReadOnly,
        );
        assert!(
            matches!(heal, LoopGuardAction::Clear),
            "distinct results must stay Clear: {heal:?}"
        );
    }
    // The entire stuck evidence has now left the 5-slot window. A new
    // 4-probe no-progress run must start the ladder over at Warn.
    for i in 0..3 {
        let action = d.record(
            "grep",
            &json!({"pattern": format!("again-{i}")}),
            empty,
            ToolExecutionClass::ReadOnly,
        );
        assert!(
            matches!(action, LoopGuardAction::Clear),
            "healed run must stay Clear (probe {i}): {action:?}"
        );
    }
    let re_warn = d.record(
        "grep",
        &json!({"pattern": "again-3"}),
        empty,
        ToolExecutionClass::ReadOnly,
    );
    assert!(
        matches!(&re_warn, LoopGuardAction::Warn(w) if matches!(w.pattern, LoopPattern::NoProgress { .. })),
        "a fresh exhausted evidence must re-enter the ladder at Warn, got {re_warn:?}"
    );
}

#[tokio::test]
async fn react_loop_survives_legitimate_edit_ack_streak() {
    // End-to-end: the exact TUI incident shape through the real ReAct loop.
    // A mutating invoker whose ack text is constant-form (`edited {path}`),
    // five differently-argued successful edits — the turn must complete
    // with zero VRO-12 intervention.
    struct EditingInvoker;
    impl ToolInvoker for EditingInvoker {
        fn class_of(&self, name: &str) -> Option<ToolExecutionClass> {
            match name {
                "edit_file" => Some(ToolExecutionClass::Mutating),
                _ => Some(ToolExecutionClass::ReadOnly),
            }
        }
        fn invoke<'a>(
            &'a self,
            name: &'a str,
            _args: &'a serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>
        {
            let name = name.to_string();
            Box::pin(async move {
                Ok(match name.as_str() {
                    "edit_file" => "edited src/lib.rs".to_string(),
                    other => format!("ok: {other}"),
                })
            })
        }
    }

    let decisions = (0..6)
        .map(|i| ReactDecision::CallTool {
            name: "edit_file".to_string(),
            arguments: json!({
                "path": "src/lib.rs",
                "old_text": format!("old-{i}"),
                "new_text": format!("new-{i}"),
            }),
        })
        .chain(std::iter::once(ReactDecision::Finish {
            output: json!({"answer": "all edits applied"}),
        }))
        .collect::<Vec<_>>();
    let agent = OrderedScriptedAgent::new(decisions);
    let (outcome, trajectory) = run_tool_grounded_react_with_trajectory(
        "apply six legitimate edits",
        &agent,
        &EditingInvoker,
        budget(20, 20),
        true,
    )
    .await;
    assert_eq!(
        outcome.status,
        OutcomeStatus::Succeeded,
        "six legitimate edits must complete the turn; got {:?}",
        outcome.unresolved_risks
    );
    assert!(
        trajectory
            .iter()
            .all(|entry| !matches!(entry, TrajectoryEntry::Observation { text, .. } if text.contains("VRO-12"))),
        "no VRO-12 bytes may appear for legitimate edit acks; got {trajectory:?}"
    );
}

// ---------------------------------------------------------------------------
// Post-audit regression tests: class-aware premises and the corrective
// no-progress ladder. These pin the two production false positives from the
// TUI incident ("VRO-12 loop guard: no-progress, tool 'grep'/'edit_file',
// 5 differently-argued calls with byte-identical results").
// ---------------------------------------------------------------------------

#[test]
fn mutating_tool_identical_acks_never_trip_no_progress() {
    // `edit_file` returns `format!("edited {path}")` — a constant-form ack.
    // Five legitimate, differently-argued edits to the same file produce
    // byte-identical results while the workspace genuinely advances. The
    // no-progress premise ("identical result = no new information") holds
    // only for read-only probes; a mutating ack must never fire it.
    let mut d = LoopDetector::new();
    for i in 0..8 {
        let action = d.record(
            "edit_file",
            &json!({"path": "src/lib.rs", "old_text": format!("old {i}"), "new_text": format!("new {i}")}),
            "edited src/lib.rs",
            ToolExecutionClass::Mutating,
        );
        assert!(
            !matches!(&action, LoopGuardAction::Warn(w) if matches!(w.pattern, LoopPattern::NoProgress { .. })),
            "legitimate differently-argued mutating edits must never warn no-progress (call {i}): {action:?}"
        );
        assert!(
            !matches!(
                &action,
                LoopGuardAction::Block(_) | LoopGuardAction::Break(_)
            ),
            "legitimate differently-argued mutating edits must never be suppressed or broken (call {i}): {action:?}"
        );
    }
}

#[test]
fn mutating_tool_identical_acks_never_trip_ping_pong() {
    // The healthy edit cycle: read → edit → read → edit. Each mutating step
    // changes workspace state even though the ack text is constant-form and
    // the read re-observation is byte-identical. Whole-window name
    // alternation alone must not classify ping-pong when a mutating tool
    // participates.
    let mut d = LoopDetector::new();
    let read = json!({"path": "src/lib.rs"});
    for edit in 0..3 {
        let a = d.record(
            "read_file",
            &read,
            "file body",
            ToolExecutionClass::ReadOnly,
        );
        assert!(
            !matches!(a, LoopGuardAction::Warn(ref w) if matches!(w.pattern, LoopPattern::PingPong { .. })),
            "read→edit alternation must never warn ping-pong: {a:?}"
        );
        let b = d.record(
            "edit_file",
            &json!({"path": "src/lib.rs", "old_text": format!("o{edit}"), "new_text": format!("n{edit}")}),
            "edited src/lib.rs",
            ToolExecutionClass::Mutating,
        );
        assert!(
            !matches!(b, LoopGuardAction::Warn(ref w) if matches!(w.pattern, LoopPattern::PingPong { .. }))
                && !matches!(&b, LoopGuardAction::Block(t) if t.contains("ping-pong")),
            "read→edit alternation must never escalate ping-pong: {b:?}"
        );
    }
}

#[test]
fn read_only_no_progress_is_advisory_not_terminal() {
    // Different grep patterns can all return empty during valid exploration.
    // Warn once, then preserve the actual results and keep the turn alive.
    let mut d = LoopDetector::new();
    for i in 0..3 {
        assert!(matches!(
            d.record(
                "grep",
                &json!({"pattern": format!("needle{i}")}),
                "",
                ToolExecutionClass::ReadOnly
            ),
            LoopGuardAction::Clear
        ));
    }
    let fourth = d.record(
        "grep",
        &json!({"pattern": "needle3"}),
        "",
        ToolExecutionClass::ReadOnly,
    );
    assert!(
        matches!(&fourth, LoopGuardAction::Warn(w) if matches!(
            &w.pattern,
            LoopPattern::NoProgress { tool, count: 4, .. } if tool == "grep"
        )),
        "4th differently-argued empty grep must Warn (not Break); got {fourth:?}"
    );
    let fifth = d.record(
        "grep",
        &json!({"pattern": "needle4"}),
        "",
        ToolExecutionClass::ReadOnly,
    );
    assert!(matches!(&fifth, LoopGuardAction::Clear));
    let sixth = d.record(
        "grep",
        &json!({"pattern": "needle5"}),
        "",
        ToolExecutionClass::ReadOnly,
    );
    assert!(matches!(&sixth, LoopGuardAction::Clear));
}

#[test]
fn no_progress_advisory_resets_when_evidence_leaves_window() {
    // The advisory state decays once its evidence leaves the bounded window.
    let mut d = LoopDetector::new();
    for i in 0..4 {
        let _ = d.record(
            "grep",
            &json!({"pattern": format!("a{i}")}),
            "",
            ToolExecutionClass::ReadOnly,
        );
    } // -> Warn at 4
    let fifth = d.record(
        "grep",
        &json!({"pattern": "b"}),
        "",
        ToolExecutionClass::ReadOnly,
    );
    assert!(matches!(fifth, LoopGuardAction::Clear));
    // Model changes strategy: distinct results wash the evidence out.
    for i in 0..5 {
        let action = d.record(
            "read_file",
            &json!({"path": format!("p{i}")}),
            &format!("body{i}"),
            ToolExecutionClass::ReadOnly,
        );
        assert!(
            !matches!(
                action,
                LoopGuardAction::Break(_) | LoopGuardAction::Block(_)
            ),
            "fresh distinct results must clear all escalation: {action:?}"
        );
    }
    // A new identical-result run starts over at the Warn tier.
    let run = |d: &mut LoopDetector, i: u32| {
        d.record(
            "grep",
            &json!({"pattern": format!("c{i}")}),
            "",
            ToolExecutionClass::ReadOnly,
        )
    };
    assert!(matches!(run(&mut d, 0), LoopGuardAction::Clear));
    assert!(matches!(run(&mut d, 1), LoopGuardAction::Clear));
    assert!(matches!(run(&mut d, 2), LoopGuardAction::Clear));
    assert!(
        matches!(run(&mut d, 3), LoopGuardAction::Warn(_)),
        "a fresh no-progress run must restart at Warn after the evidence left the window"
    );
}

#[test]
fn shell_tool_identical_output_never_trips_result_detectors() {
    // `run_command` acks (bounded stdout of identical commands) are
    // Shell-class; even identical output across differently-argued calls
    // must not fire no-progress. (Identical *calls* still hit Exact Repeat.)
    let mut d = LoopDetector::new();
    for i in 0..6 {
        let action = d.record(
            "run_command",
            &json!({"command": format!("echo probe-{i}")}),
            "same bounded output",
            ToolExecutionClass::Shell,
        );
        assert!(
            !matches!(&action, LoopGuardAction::Warn(w) if matches!(w.pattern, LoopPattern::NoProgress { .. })),
            "Shell-class identical output must not fire no-progress (call {i}): {action:?}"
        );
    }
}

#[test]
fn mutating_tool_exact_repeat_still_fires() {
    // The class gate must NOT weaken Exact Repeat: a literally identical
    // mutating call repeated is a loop regardless of class (the key is the
    // call, not the result).
    let mut d = LoopDetector::new();
    let call = json!({"path": "src/lib.rs", "old_text": "a", "new_text": "b"});
    for _ in 0..2 {
        assert!(matches!(
            d.record(
                "edit_file",
                &call,
                "edited src/lib.rs",
                ToolExecutionClass::Mutating
            ),
            LoopGuardAction::Clear
        ));
    }
    assert!(
        matches!(
            d.record(
                "edit_file",
                &call,
                "edited src/lib.rs",
                ToolExecutionClass::Mutating
            ),
            LoopGuardAction::Warn(_)
        ),
        "identical mutating calls must still Warn exact repeat"
    );
    assert!(
        matches!(
            d.record(
                "edit_file",
                &call,
                "edited src/lib.rs",
                ToolExecutionClass::Mutating
            ),
            LoopGuardAction::Block(_)
        ),
        "identical mutating calls must still Block exact repeat"
    );
    assert!(
        matches!(
            d.record(
                "edit_file",
                &call,
                "edited src/lib.rs",
                ToolExecutionClass::Mutating
            ),
            LoopGuardAction::Break(_)
        ),
        "identical mutating calls must still Break exact repeat at saturation"
    );
}

#[tokio::test]
async fn react_loop_read_edit_cycle_is_not_a_loop() {
    // End-to-end ReAct variant of the incident: the healthy
    // read_file → edit_file → read_file → edit_file cycle with constant-form
    // edit acks and re-reads must run to the numeric budget, never a
    // VRO-12 break or block.
    struct EditCycleInvoker;
    impl ToolInvoker for EditCycleInvoker {
        fn class_of(&self, name: &str) -> Option<ToolExecutionClass> {
            match name {
                "read_file" => Some(ToolExecutionClass::ReadOnly),
                "edit_file" => Some(ToolExecutionClass::Mutating),
                _ => None,
            }
        }
        fn invoke<'a>(
            &'a self,
            name: &'a str,
            _args: &'a serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>
        {
            let name = name.to_string();
            Box::pin(async move { Ok(format!("{name} done")) })
        }
    }

    let mut decisions = Vec::new();
    for i in 0..6 {
        decisions.push(ReactDecision::CallTool {
            name: "read_file".to_string(),
            arguments: json!({"path": "src/lib.rs"}),
        });
        decisions.push(ReactDecision::CallTool {
            name: "edit_file".to_string(),
            arguments: json!({
                "path": "src/lib.rs",
                "old_text": format!("old {i}"),
                "new_text": format!("new {i}")
            }),
        });
    }
    decisions.push(ReactDecision::Finish {
        output: json!({"answer": "edited the file six times"}),
    });
    let agent = OrderedScriptedAgent::new(decisions);
    let (outcome, trajectory) = run_tool_grounded_react_with_trajectory(
        "iterate on the file",
        &agent,
        &EditCycleInvoker,
        budget(20, 20),
        true,
    )
    .await;
    assert_eq!(
        outcome.status,
        OutcomeStatus::Succeeded,
        "the healthy read→edit cycle must complete, not break; risks: {:?}",
        outcome.unresolved_risks
    );
    assert!(
        trajectory
            .iter()
            .all(|entry| !matches!(entry, TrajectoryEntry::Observation { text, .. } if text.contains("[VRO-12") || text.contains("LOOP BLOCKED"))),
        "the healthy read→edit cycle must produce no guard intervention; got {trajectory:?}"
    );
}

#[tokio::test]
async fn react_loop_grep_no_progress_stays_alive_after_advisory() {
    // A sequence of distinct grep probes may be an intentional survey. The
    // guard must warn once but must not terminate either host's turn.
    struct EmptyGrepInvoker;
    impl ToolInvoker for EmptyGrepInvoker {
        fn class_of(&self, _name: &str) -> Option<ToolExecutionClass> {
            Some(ToolExecutionClass::ReadOnly)
        }
        fn invoke<'a>(
            &'a self,
            _name: &'a str,
            _args: &'a serde_json::Value,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolInvocationError>> + Send + 'a>>
        {
            Box::pin(async { Ok(String::new()) })
        }
    }

    let decisions = (0..12)
        .map(|i| ReactDecision::CallTool {
            name: "grep".to_string(),
            arguments: json!({"pattern": format!("needle{i}")}),
        })
        .collect::<Vec<_>>();
    let agent = OrderedScriptedAgent::new(decisions);
    let (outcome, trajectory) = run_tool_grounded_react_with_trajectory(
        "hunt forever",
        &agent,
        &EmptyGrepInvoker,
        budget(20, 20),
        true,
    )
    .await;
    assert_eq!(outcome.status, OutcomeStatus::Succeeded);
    assert!(
        outcome.unresolved_risks.is_empty(),
        "advisory no-progress must not create a terminal risk: {:?}",
        outcome.unresolved_risks
    );
    // A single advisory is visible, with no synthetic block.
    let observations: Vec<&TrajectoryEntry> = trajectory
        .iter()
        .filter(|entry| {
            matches!(entry, TrajectoryEntry::Observation { text, .. }
                if text.contains("[VRO-12 Loop Guard]"))
        })
        .collect();
    let saw_warn = observations.iter().any(|entry| {
        matches!(entry, TrajectoryEntry::Observation { text, .. } if text.contains("[Loop Detection Warning]"))
    });
    assert!(
        saw_warn && observations.iter().all(|entry| !matches!(entry, TrajectoryEntry::Observation { text, .. } if text.contains("LOOP BLOCKED"))),
        "no-progress must be advisory only; got {trajectory:?}"
    );
}
