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
//! - **No-Progress** — the same tool returns byte-identical output across
//!   ≥ 4 *differently-argued* probes. Counted with `filter`, not
//!   `take_while`, so an unrelated interleaved call does not reset the
//!   streak (the reference repo's 43-near-duplicate-calls lesson).
//!
//! Escalation ladder: [`LoopGuardAction::Warn`] (nudge observation appended
//! to the trajectory) → [`LoopGuardAction::Block`] (result replaced, the
//! blocked attempt does not consume a `max_tool_calls` unit, mirroring the
//! Read-Before-Write precedent) → [`LoopGuardAction::Break`] (circuit
//! breaker; the turn halts with `BudgetExceeded` and a named risk).
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
    /// Circuit breaker: halt the turn (`BudgetExceeded`) with the named
    /// cause as an `unresolved_risks` entry (PRD §3).
    Break(String),
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
}

/// In-window memory of the last emitted Warn, so a *persisting* pattern
/// escalates to Block on the next record. Resets when the evidence pattern
/// leaves the window (PRD §3: no cross-turn memory, no growth).
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

/// Hash of a JSON arguments value. `serde_json::to_string` is canonical here
/// because the workspace `serde_json` has no `preserve_order` feature
/// (`Map` is a `BTreeMap`): object keys serialize sorted, so key order does
/// not change the digest. Serialization failure (non-finite floats) hashes
/// the empty payload — still deterministic.
fn hash_args(args: &Value) -> u64 {
    let canonical = serde_json::to_string(args).unwrap_or_default();
    hash_bytes(b"vro12-args", canonical.as_bytes())
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
    pub fn record(&mut self, name: &str, args: &Value, result: &str) -> LoopGuardAction {
        let record = ToolCallRecord {
            name: name.to_string(),
            args_hash: hash_args(args),
            result_hash: hash_result(result),
        };

        // Sliding window: evict the oldest once at capacity, then append.
        if self.window.len() >= LOOP_WINDOW_SIZE {
            self.window.pop_front();
        }
        self.window.push_back(record);

        // Detector 1 — Exact Repeat (absolute thresholds; PRD §3).
        if let Some(action) = self.detect_exact_repeat() {
            self.retain_warn_state();
            return action;
        }
        // Detector 2 — Ping-Pong (Warn, then Block on persisting pattern).
        if let Some(action) = self.detect_ping_pong() {
            self.retain_warn_state();
            return action;
        }
        // Detector 3 — No-Progress (Warn at 4; saturation at 5 is Break —
        // the Block tier is subsumed by the Break threshold, documented in
        // PRD §3).
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
            Some(LoopGuardAction::Break(format!(
                "VRO-12 loop guard: {}, tool '{}', {} consecutive identical calls \
                 (window saturated)",
                pattern.name(),
                last.name,
                run
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
    /// count == window size ⇒ Break · count ≥ 4 ⇒ Warn.
    fn detect_no_progress(&mut self) -> Option<LoopGuardAction> {
        if self.window.len() < NO_PROGRESS_MIN {
            return None;
        }
        let last = self.window.back()?;
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
        if count >= LOOP_WINDOW_SIZE {
            Some(LoopGuardAction::Break(format!(
                "VRO-12 loop guard: {}, tool '{}', {} differently-argued calls with \
                 byte-identical results (window saturated)",
                pattern.name(),
                last.name,
                count
            )))
        } else {
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
