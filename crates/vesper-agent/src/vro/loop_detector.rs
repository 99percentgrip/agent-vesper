//! VRO-12 — Result-aware loop detection guard (PRD
//! `docs/result-aware-loop-detection-prd.md`).
//!
//! A deterministic, allocation-bounded detector for token-burning tool
//! loops, evaluated after every **successful** OBSERVE step of the VRO ReAct
//! loop ([`super::react`]). Three patterns are classified over a sliding
//! window of the last [`LOOP_WINDOW_SIZE`] executed `(tool, args-hash,
//! result-hash)` triples:
//!
//! - **Exact Repeat** — trailing run of identical `(name, args_hash)` calls
//!   (result is identical by construction of the run in the common case; the
//!   key is the call, per PRD §2.2).
//! - **Ping-Pong** — the entire window alternates two distinct tool names
//!   (`A, B, A, B, A`) without new state. A name-level pattern is the
//!   signal; args may vary.
//! - **No-Progress** — the same **read-only** tool returns byte-identical
//!   output across ≥ 4 *differently-argued* probes. Counted with `filter`,
//!   not `take_while`, so an unrelated interleaved call does not reset the
//!   streak (the reference repo's 43-near-duplicate-calls lesson).
//!
//! Escalation ladder: [`LoopGuardAction::Warn`] (nudge observation appended
//! to the trajectory) → [`LoopGuardAction::Block`] (result replaced, the
//! blocked attempt does not consume a `max_tool_calls` unit, mirroring the
//! Read-Before-Write precedent) → [`LoopGuardAction::Break`] (circuit
//! breaker; the turn halts with `BudgetExceeded` and a named risk).
//!
//! ## Class-aware premises (post-audit correction)
//!
//! The No-Progress and Ping-Pong detectors reason from "byte-identical
//! results ⇒ no new information reached the model". That premise holds only
//! for **read-only** tools. Mutating and shell tools return constant-form
//! acknowledgments by design (`edited {path}`, `wrote N bytes to {path}`):
//! five legitimate, differently-argued edits to the same file produce
//! byte-identical acks while the workspace state genuinely advances, and
//! `grep` legitimately returns the empty string for every non-matching
//! pattern. Feeding those acks into the no-progress key produced fatal
//! false-positive `Break`s in real coding turns (the TUI loop-detector
//! incident). Both detectors are therefore gated on the recorded execution
//! class: only windows whose matching records are
//! [`ToolExecutionClass::ReadOnly`] can classify No-Progress or Ping-Pong.
//! Exact Repeat is *not* gated — an identical call repeated is a loop
//! regardless of class, and its key does not include the result.
//!
//! No-Progress is deliberately advisory: it injects one Warn at 4 matching
//! probes, then lets the turn continue. Different searches with the same
//! empty result are ordinary repository exploration, not proof that a turn
//! is unsafe. The hard tool budget remains the terminal safety ceiling. The
//! execution class comes from the caller's registry (the
//! same [`ToolExecutionClass`] the permission gate uses), so a classless
//! caller can still construct the guard; the class-aware detectors treat
//! an unknown class as mutating (fail-open for legitimate work, never
//! fail-closed into a false `Break`).
//!
//! ## Determinism (PRD C5)
//!
//! No clocks, no randomness, no [`DefaultHasher`] (not stability-guaranteed
//! across Rust releases). Hashes are SHA-256 digests truncated to `u64`
//! (first 8 bytes, big-endian) via the workspace `sha2` dependency — the
//! same crate VRO-7 uses for deterministic procedure IDs. `args` are hashed
//! over `serde_json::to_string(args)`; under the workspace's default
//! `serde_json` feature set `Map` is a `BTreeMap`, so object keys serialize
//! in sorted order and two calls differing only in key order hash
//! identically (pinned by test). `args` and `result` digests are
//! domain-separated with distinct prefix bytes so they cannot cross-collide.
//!
//! ## Zero-breakage (PRD C3/C4)
//!
//! This module is pure (no I/O, no provider handles, `#![forbid(unsafe_code)]`
//! inherited) and is wired into exactly one place: the OBSERVE step of
//! [`run_tool_grounded_react_with_trajectory`](super::react). Failed
//! invocations and Read-Before-Write rejections are **never recorded** — a
//! failure already produces a structured observation the model must react
//! to, and recording failures would conflate "model stuck" with "tool
//! broken".

use std::collections::VecDeque;

use serde_json::Value;
use sha2::{Digest, Sha256};
use vesper_domain::ToolExecutionClass;

/// Sliding-window capacity (directive-fixed, PRD C2).
pub const LOOP_WINDOW_SIZE: usize = 5;

/// Trailing identical `(name, args_hash)` run that triggers a [`LoopGuardAction::Warn`].
pub const EXACT_REPEAT_WARN: usize = 3;

/// Trailing identical `(name, args_hash)` run that triggers a [`LoopGuardAction::Block`].
pub const EXACT_REPEAT_BLOCK: usize = 4;

/// Minimum distinct-args / identical-result calls for a No-Progress [`LoopGuardAction::Warn`].
pub const NO_PROGRESS_MIN: usize = 4;

/// Minimum window length before Ping-Pong can be classified (two full cycles).
const PING_PONG_MIN_WINDOW: usize = 4;

/// Prefix action taken by the ReAct loop after recording a successful call.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LoopGuardAction {
    /// No pattern detected — continue normally.
    #[default]
    Clear,
    /// Suspicious pattern: append the nudge observation after the real
    /// result; the turn continues with full budget accounting.
    Warn(LoopWarning),
    /// The result must be replaced with the override text; the blocked
    /// attempt does NOT consume a `max_tool_calls` unit (PRD §4).
    Block(String),
    /// Circuit breaker: halt the turn (`BudgetExceeded` / `LoopDetected`)
    /// with the named cause as an `unresolved_risks` entry (PRD §3).
    ///
    /// The payload is a typed [`LoopBreak`] so callers can classify the
    /// outcome structurally instead of matching on message prefixes.
    Break(LoopBreak),
}

/// Which pattern fired, with the evidence counts used to phrase the nudge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopPattern {
    /// Trailing run of identical `(name, args_hash)` calls.
    ExactRepeat {
        /// Tool name.
        tool: String,
        /// Length of the trailing identical run.
        run: usize,
    },
    /// Whole-window alternation of two distinct tool names.
    PingPong {
        /// First alternating tool name.
        a: String,
        /// Second alternating tool name.
        b: String,
    },
    /// Same tool, byte-identical results, differing arguments.
    NoProgress {
        /// Tool name.
        tool: String,
        /// Calls with the identical result.
        count: usize,
        /// Distinct argument hashes among them.
        distinct_args: usize,
    },
}

impl LoopPattern {
    /// Human name used in the Break risk note (PRD §3: `<pattern>`).
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::ExactRepeat { .. } => "exact repeat",
            Self::PingPong { .. } => "ping-pong",
            Self::NoProgress { .. } => "no-progress",
        }
    }
}

/// Why the detector broke the turn: the pattern plus the tool(s) involved.
/// Carried alongside the risk note so hosts and tests can classify a
/// [`LoopGuardAction::Break`] by type instead of matching on message text
/// (PRD §3 named-cause contract; the text remains for human-readable
/// `unresolved_risks`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopBreak {
    /// Pattern that fired.
    pub pattern: LoopPattern,
    /// Full human-readable risk note (`"VRO-12 loop guard: …"`), suitable
    /// for `ReasoningOutcome::unresolved_risks` /
    /// `AgentLoopError::LoopDetected`.
    pub message: String,
}

impl LoopBreak {
    /// Builds a Break payload from a pattern and a risk-note suffix.
    fn new(pattern: LoopPattern, detail: String) -> Self {
        Self {
            pattern,
            message: format!("VRO-12 loop guard: {detail}"),
        }
    }
}

/// A Warn escalation: the pattern plus the message the loop appends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopWarning {
    /// Pattern that fired.
    pub pattern: LoopPattern,
    /// Full nudge text (already prefixed `[Loop Detection Warning]`).
    pub message: String,
}

/// One recorded successful tool invocation inside the sliding window
/// (PRD §2.2 `ToolCallRecord`).
#[derive(Debug, Clone)]
struct ToolCallRecord {
    name: String,
    args_hash: u64,
    result_hash: u64,
    /// Execution class at dispatch time. Result-aware detectors (Ping-Pong,
    /// No-Progress) may only fire on read-only tools: mutating/shell tools
    /// return constant-form acknowledgments whose text does not encode the
    /// state change, so "byte-identical result" cannot mean "no progress"
    /// for them. Exact Repeat ignores the class — an identical call is a
    /// loop regardless.
    class: ToolExecutionClass,
}

/// In-window memory of the last emitted advisory/intervention. Resets when
/// the evidence pattern leaves the window (PRD §3: no cross-turn memory, no
/// growth).
#[derive(Debug, Clone, PartialEq, Eq)]
enum WarnState {
    ExactRepeat { name: String, args_hash: u64 },
    PingPong { a: String, b: String },
    NoProgress { name: String, result_hash: u64 },
}

/// SHA-256 of `prefix || payload`, truncated to the first 8 bytes big-endian.
fn hash_bytes(prefix: &[u8], payload: &[u8]) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(payload);
    let digest = hasher.finalize();
    let mut first8 = [0u8; 8];
    first8.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(first8)
}

/// Hash of a JSON arguments value.
///
/// Canonical form is produced by an explicit recursive walk that sorts object
/// keys and emits no whitespace — the same approach as
/// [`crate::vro::strategies::normalize_output`]. This is REQUIRED: under
/// `--all-features` (the `cargo xtask verify` gate), an upstream dev-only
/// dependency chain enables `serde_json`'s `preserve_order` feature, which
/// makes `Value::Object` insertion-ordered, so `serde_json::to_string` is NOT
/// key-order-stable across feature configurations. The explicit walk is
/// independent of that flag (regression-pinned by
/// `args_hash_is_invariant_under_object_key_reordering`). Nested objects and
/// arrays recurse. Serialization failure (non-finite floats) hashes the empty
/// payload — still deterministic.
fn hash_args(args: &Value) -> u64 {
    hash_bytes(b"vro12-args", canonical_json(args).as_bytes())
}

/// Feature-independent canonical JSON: recursively sorted object keys, no
/// whitespace. Mirrors `normalize_output`'s canonicalization.
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            let mut out = String::from("{");
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| "\"\"".into()));
                out.push(':');
                out.push_str(&canonical_json(&map[*key]));
            }
            out.push('}');
            out
        }
        Value::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&canonical_json(item));
            }
            out.push(']');
            out
        }
        scalar => serde_json::to_string(scalar).unwrap_or_default(),
    }
}

/// Hash of a tool's bounded result text.
fn hash_result(result: &str) -> u64 {
    hash_bytes(b"vro12-result", result.as_bytes())
}

/// Result-aware loop detector (PRD §2–§4). Construct one per ReAct turn and
/// call [`record`](Self::record) after every successful tool observation.
#[derive(Debug, Clone, Default)]
pub struct LoopDetector {
    window: VecDeque<ToolCallRecord>,
    warned: Option<WarnState>,
}

impl LoopDetector {
    /// Creates a detector with an empty window and no escalation state.
    #[must_use]
    pub fn new() -> Self {
        let mut window = VecDeque::with_capacity(LOOP_WINDOW_SIZE);
        window.clear();
        Self {
            window,
            warned: None,
        }
    }

    /// Number of records currently retained (≤ [`LOOP_WINDOW_SIZE`]).
    #[must_use]
    pub fn len(&self) -> usize {
        self.window.len()
    }

    /// Whether no records are retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.window.is_empty()
    }

    /// Records one **successful** tool invocation and classifies the window.
    ///
    /// Detectors run in fixed escalation order (most severe classification
    /// first, PRD §3): Exact Repeat → Ping-Pong → No-Progress. Failed
    /// invocations and Read-Before-Write rejections must NOT be recorded.
    ///
    /// `execution_class` is the recorded authority class of the executed
    /// tool. It feeds the result-aware detectors' premise gate: Ping-Pong
    /// and No-Progress reason from "identical results ⇒ no new information",
    /// which holds only for read-only tools, so any window entry classified
    /// Mutating/Shell/Process/NestedWorkflow blocks those detectors (see
    /// the module docs).
    pub fn record(
        &mut self,
        name: &str,
        args: &Value,
        result: &str,
        execution_class: ToolExecutionClass,
    ) -> LoopGuardAction {
        let record = ToolCallRecord {
            name: name.to_string(),
            args_hash: hash_args(args),
            result_hash: hash_result(result),
            class: execution_class,
        };

        // Sliding window: evict the oldest once at capacity, then append.
        if self.window.len() >= LOOP_WINDOW_SIZE {
            self.window.pop_front();
        }
        self.window.push_back(record);

        // Detector 1 — Exact Repeat (absolute thresholds; PRD §3). Not
        // class-gated: an identical call repeated is a loop regardless of
        // the tool's authority class.
        if let Some(action) = self.detect_exact_repeat() {
            self.retain_warn_state();
            return action;
        }
        // Detector 2 — Ping-Pong (Warn, then Block on persisting pattern).
        // Class-gated: an alternating window that contains a mutating or
        // shell tool is making state changes, not thrashing.
        if let Some(action) = self.detect_ping_pong() {
            self.retain_warn_state();
            return action;
        }
        // Detector 3 — No-Progress (read-only tools only). It is advisory:
        // one warning per evidence window, never a Block or Break. Distinct
        // searches can legitimately have identical empty output.
        if let Some(action) = self.detect_no_progress() {
            self.retain_warn_state();
            return action;
        }

        self.retain_warn_state();
        LoopGuardAction::Clear
    }

    /// Exact Repeat: trailing run of identical `(name, args_hash)`.
    /// run ≥ 5 ⇒ Break · run ≥ 4 ⇒ Block · run ≥ 3 ⇒ Warn.
    fn detect_exact_repeat(&mut self) -> Option<LoopGuardAction> {
        let last = self.window.back()?;
        let run = self
            .window
            .iter()
            .rev()
            .take_while(|r| r.name == last.name && r.args_hash == last.args_hash)
            .count();

        if run < EXACT_REPEAT_WARN {
            return None;
        }

        let pattern = LoopPattern::ExactRepeat {
            tool: last.name.clone(),
            run,
        };
        if run >= LOOP_WINDOW_SIZE {
            Some(LoopGuardAction::Break(LoopBreak::new(
                pattern,
                format!(
                    "exact repeat, tool '{}', {run} consecutive identical calls \
                     (window saturated)",
                    last.name
                ),
            )))
        } else if run >= EXACT_REPEAT_BLOCK {
            Some(LoopGuardAction::Block(format!(
                "[SYSTEM OVERRIDE: LOOP BLOCKED. YOU MUST CHANGE STRATEGY.] \
                 [VRO-12 Loop Guard] '{}' suppressed: {} ({} identical consecutive calls). \
                 State what the repeated results prove, then take a different action or Finish.",
                last.name,
                pattern.name(),
                run
            )))
        } else {
            let was_warned = matches!(
                &self.warned,
                Some(WarnState::ExactRepeat { name, args_hash })
                    if *name == last.name && *args_hash == last.args_hash
            );
            self.warned = Some(WarnState::ExactRepeat {
                name: last.name.clone(),
                args_hash: last.args_hash,
            });
            let message = if was_warned {
                format!(
                    "[Loop Detection Warning] [VRO-12 Loop Guard] You were already warned: \
                     '{tool}' has now been called {run} times in a row with identical arguments \
                     and the same result. The next repeat will be blocked.",
                    tool = last.name
                )
            } else {
                format!(
                    "[Loop Detection Warning] [VRO-12 Loop Guard] You have called '{tool}' \
                     {run} times in a row with identical arguments and it returned the same \
                     result. Repeating it again will be blocked. State what the repeated \
                     results prove, then take a different action or Finish.",
                    tool = last.name
                )
            };
            Some(LoopGuardAction::Warn(LoopWarning { pattern, message }))
        }
    }

    /// Ping-Pong: the whole window alternates two distinct tool names.
    /// PRD §3 adaptation note: the 5-entry window can see at most 2 full
    /// cycles + 1 confirming entry, so the reference's cycle-count
    /// thresholds are replaced by whole-window pattern matching plus
    /// in-window escalation state.
    fn detect_ping_pong(&mut self) -> Option<LoopGuardAction> {
        if self.window.len() < PING_PONG_MIN_WINDOW {
            return None;
        }
        // Class gate: the "no new state" premise holds only when every
        // alternating tool is read-only. A read→edit→read→edit window is
        // the healthy edit cycle, not thrashing: each mutating step
        // advances workspace state even though its constant-form ack text
        // is byte-identical.
        if !self
            .window
            .iter()
            .all(|r| r.class == ToolExecutionClass::ReadOnly)
        {
            return None;
        }
        let names: Vec<&str> = self.window.iter().map(|r| r.name.as_str()).collect();
        let (a, b) = (names[0], names[1]);
        if a == b {
            return None;
        }
        // names[0] is the OLDEST entry (front). The alternating check is
        // order-independent as long as parity is consistent across the
        // whole window.
        let alternates = names
            .iter()
            .enumerate()
            .all(|(i, n)| if i % 2 == 0 { *n == a } else { *n == b });

        if !alternates {
            return None;
        }

        let pattern = LoopPattern::PingPong {
            a: a.to_string(),
            b: b.to_string(),
        };
        // Persisting pattern after a prior Warn for the same pair ⇒ Block.
        // Pair identity is order-insensitive: as the window slides, the
        // parity anchor flips (the older entry of the pair evicts first),
        // swapping which name lands at index 0. Storing/ comparing the pair
        // unordered keeps the escalation state stable across slides.
        let warned_pair = matches!(
            &self.warned,
            Some(WarnState::PingPong { a: wa, b: wb })
                if (*wa == a && *wb == b) || (*wa == b && *wb == a)
        );
        if warned_pair {
            return Some(LoopGuardAction::Block(format!(
                "[SYSTEM OVERRIDE: LOOP BLOCKED. YOU MUST CHANGE STRATEGY.] \
                 [VRO-12 Loop Guard] '{}'/'{}' alternation suppressed: {} has persisted since \
                 the warning. Choose a different strategy, change the arguments substantively, \
                 or Finish with what you have.",
                a,
                b,
                pattern.name()
            )));
        }
        self.warned = Some(WarnState::PingPong {
            a: a.to_string(),
            b: b.to_string(),
        });
        Some(LoopGuardAction::Warn(LoopWarning {
            pattern,
            message: format!(
                "[Loop Detection Warning] [VRO-12 Loop Guard] '{a}' and '{b}' have alternated \
                 for the entire recent window without new state. Choose a different strategy, \
                 change the arguments substantively, or Finish with what you have."
            ),
        }))
    }

    /// No-Progress: the last tool's `(name, result_hash)` appears ≥ 4 times
    /// in the window with ≥ 2 distinct argument hashes (identical args are
    /// detector 1's territory). Counted with `filter` across the whole
    /// window so interleaved unrelated calls do not reset the streak.
    ///
    /// **Read-only tools only.** Mutating/shell acks are constant-form text
    /// that does not encode the state change, so identical acks do not mean
    /// no progress for them.
    ///
    /// Advisory: count ≥ 4 ⇒ one Warn for the matching evidence window.
    /// Subsequent distinct probes continue normally. The numeric tool budget,
    /// not a heuristic about equal search output, is the terminal ceiling.
    fn detect_no_progress(&mut self) -> Option<LoopGuardAction> {
        if self.window.len() < NO_PROGRESS_MIN {
            return None;
        }
        let last = self.window.back()?;
        // Class gate: only a read-only tool's result text is information
        // the model did not already have; a mutating ack is not.
        if last.class != ToolExecutionClass::ReadOnly {
            return None;
        }
        let matching: Vec<&ToolCallRecord> = self
            .window
            .iter()
            .filter(|r| r.name == last.name && r.result_hash == last.result_hash)
            .collect();
        let count = matching.len();
        if count < NO_PROGRESS_MIN {
            return None;
        }
        let distinct_args: usize = {
            let mut hashes: Vec<u64> = matching.iter().map(|r| r.args_hash).collect();
            hashes.sort_unstable();
            hashes.dedup();
            hashes.len()
        };
        if distinct_args < 2 {
            // All identical args — exact-repeat territory (reference
            // separation, kept per PRD §3).
            return None;
        }

        let pattern = LoopPattern::NoProgress {
            tool: last.name.clone(),
            count,
            distinct_args,
        };
        let warned_same = matches!(
            &self.warned,
            Some(WarnState::NoProgress { name, result_hash })
                if *name == last.name && *result_hash == last.result_hash
        );
        if warned_same {
            // Repository investigation commonly tries several patterns that
            // all yield no matches. Preserve the result and leave the turn
            // running after its single advisory rather than manufacturing a
            // failure from equal output.
            None
        } else {
            // First tier: corrective nudge appended to the real result.
            self.warned = Some(WarnState::NoProgress {
                name: last.name.clone(),
                result_hash: last.result_hash,
            });
            Some(LoopGuardAction::Warn(LoopWarning {
                pattern,
                message: format!(
                    "[Loop Detection Warning] [VRO-12 Loop Guard] '{tool}' has returned \
                     byte-identical output across {count} differently-argued probes. The \
                     information source is exhausted. Stop probing it; reason from the \
                     observations already collected, or Finish.",
                    tool = last.name
                ),
            }))
        }
    }

    /// Drops the escalation state when its evidence pattern is no longer in
    /// the window (PRD §3: escalation resets when the evidence leaves).
    fn retain_warn_state(&mut self) {
        let Some(state) = self.warned.clone() else {
            return;
        };
        let still_present = match state {
            WarnState::ExactRepeat { name, args_hash } => self
                .window
                .iter()
                .any(|r| r.name == name && r.args_hash == args_hash),
            WarnState::PingPong { .. } => {
                // The state survives only while the window still fully
                // alternates the same pair (re-evaluate shape-only).
                if self.window.len() < PING_PONG_MIN_WINDOW {
                    false
                } else {
                    let names: Vec<&str> = self.window.iter().map(|r| r.name.as_str()).collect();
                    let (a, b) = (names[0], names[1]);
                    a != b
                        && names
                            .iter()
                            .enumerate()
                            .all(|(i, n)| if i % 2 == 0 { *n == a } else { *n == b })
                }
            }
            WarnState::NoProgress { name, result_hash } => {
                // The advisory remains spent while its matching evidence is
                // still in the bounded window, so one investigation cluster
                // produces one nudge rather than flooding the transcript.
                let count = self
                    .window
                    .iter()
                    .filter(|r| r.name == name && r.result_hash == result_hash)
                    .count();
                count >= NO_PROGRESS_MIN
            }
        };
        if !still_present {
            self.warned = None;
        }
    }
}
