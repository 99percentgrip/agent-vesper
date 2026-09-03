# VRO-12 — Result-Aware Loop Detection (PRD)

Status: IMPLEMENTED · Phase VRO-12 · Owner: `crates/vesper-agent`
Audit: 2026-08-30 evidence-based audit complete — see
`docs/result-aware-loop-detection-audit-report.md` and §8 below.
Predecessor: VRO-11 (ADR 0017, VesperLens) · Family: Vesper Reasoning
Orchestrator (`docs/agent-vesper-reasoning-orchestrator-prd.md`)

Reference model: `zeroclaw` @ `d5b69f54ee8eab90c5142093cc87010b7ee3bf62`,
`crates/zeroclaw-runtime/src/agent/loop_detector.rs` (data-only structural
reference; no code copied, no dependency introduced; the repo was never
executed — read exclusively via `sed`/`grep`).

---

## 1. Executive Summary

A tool-grounded agent that repeats itself is the most expensive failure mode
in the harness: every redundant THINK→ACT→OBSERVE cycle burns provider
tokens, wall-clock budget (`max_wall_time_ms`, VRO-9), and tool-call budget
(`max_tool_calls`) while producing zero state change. The existing Vesper
ReAct loop (`crates/vesper-agent/src/vro/react.rs`) halts only on *quantity*
ceilings — iteration counts — which fire after the damage is done and say
nothing about *quality*. An agent that calls the same tool with the same
arguments five times, or ping-pongs between two tools, or retries a failing
probe with different arguments but byte-identical results, runs to its
numeric ceiling at full token cost.

VRO-12 adds **result-aware loop detection**: a deterministic, allocation-
bounded sliding window over the last **5** executed (tool, args-hash,
result-hash) triples, evaluated after every OBSERVE step of the VRO ReAct
loop. Three patterns are classified — **Exact Repeat**, **Ping-Pong**, and
**No-Progress**. Exact Repeat and Ping-Pong use the corrective *warn* →
*block* → *break* ladder; No-Progress is a one-warning advisory because
equal output from distinct repository probes is not sufficient evidence of
an unsafe turn.

Objective: **stop token-burning loops at the first classifiable evidence,
before the numeric ceilings fire, with zero behavior change to any path that
is not the VRO-orchestrated ReAct loop.**

Measured floor at draft time: **1193 test functions workspace-wide (336 in
`vesper-agent`)**, all green at HEAD. The directive's "1028+ tests remain
green" is already superseded; §6 encodes the real gate.

---

## 2. Architectural Constraints

### 2.1 Hard constraints

| # | Constraint | Enforcement |
|---|---|---|
| C1 | **Zero new dependencies.** `VecDeque` is `std::collections`; result/args hashing uses `sha2`, already a `vesper-agent` dependency (VRO-7 deterministic procedure IDs, `crates/vesper-agent/Cargo.toml`). | `Cargo.toml` diff must be empty; `cargo xtask architecture` dependency-direction scan must stay green. |
| C2 | **Window = `VecDeque<ToolCallRecord>` holding the last 5 executed tools** (directive-fixed). | Unit test pins capacity: 8 recorded calls ⇒ internal length ≤ 5. |
| C3 | **Host parity:** the same detector protects both ReAct and the shared direct `AgentLoop` used by ACP and TUI; non-ReAct orchestration strategies remain unchanged. | ReAct integration tests plus a direct-loop regression prove both paths stop a saturated repeat before the numeric cap. |
| C4 | **No loop-mechanics seam changes.** `ReactAgent`, `ToolInvoker`, and every public orchestrator signature stay byte-identical. The intervention enters through the existing trajectory channel (`TrajectoryEntry::Observation`) — the same self-correction channel Read-Before-Write already uses. *(Audit note: `LoopGuardAction::Break` was widened from `Break(String)` to `Break(LoopBreak)` — a payload refinement inside the guard's own module that removes the last string-prefix classification coupling; every trait, orchestrator entry point, and `pub fn` signature on `ReactAgent`/`ToolInvoker`/`run_tool_grounded_react*` is unchanged.)* | `git diff` on trait/`pub fn` signatures must be empty; the only type added is `LoopBreak` inside `loop_detector.rs`. |
| C5 | **Determinism.** No clocks, no randomness, no `DefaultHasher` (not stability-guaranteed across Rust releases). Hashes are SHA-256 digests truncated to `u64` (first 8 bytes, big-endian). | Unit tests are pure-function; no `Instant`/`SystemTime` in `src/vro/loop_detector.rs`. |
| C6 | **Allocation-bounded.** One `ToolCallRecord` push per tool call; window eviction is `pop_front`; no per-record heap growth beyond the fixed 5-entry ring. Hash input is the bounded result `String` the invoker already produced. | Review-only (mirrors VRO-7 `hash_value` precedent: allocation shape argued in code, equality classes pinned by test). |

### 2.2 Detection keys

`ToolCallRecord { name: String, args_hash: u64, result_hash: u64 }`

- **args_hash** = `SHA-256(canonical_json(args))[0..8]`. Canonical form is an
  **explicit recursive walk** (`canonical_json`) that sorts object keys and
  emits no whitespace — mirroring `normalize_output`. This is REQUIRED, not
  an optimization: under `--all-features` (the `cargo xtask verify` gate), a
  dev-only dependency chain (`vesper-testkit → jsonschema →
  serde_json/preserve_order`) makes `Map` insertion-ordered, so plain
  `serde_json::to_string` is **not** key-order-stable across feature
  configurations and two calls differing only in key order would hash
  differently. The explicit walk is feature-independent and pinned by a
  guard test. (Divergence from the reference, which hand-rolls a streaming
  sorted-key walker for the same reason.)
- **result_hash** = `SHA-256(result_text)[0..8]`. Numeric/text distinction
  is inherited from JSON serialization (`0` vs `0.0` vs `"0"` serialize to
  distinct strings — the same collision class the reference patches with
  length-prefixed canonical text; serialization gives it to us for free).
- **Recorded only on successful invocations.** A failed invocation
  (`ToolInvocationError::*`) already produces a structured failure
  observation the model must react to; recording failures would conflate
  "model stuck" with "tool broken". Mirrors the reference
  (`results_collect.rs`: failures ⇒ `LoopDetectionResult::Ok`, no record).
- **Read-Before-Write rejections are never recorded.** They are synthesized
  by the loop before reaching the invoker (react.rs: they do not consume
  `max_tool_calls`); they are policy feedback, not executed tools.

### 2.3 Placement

New module `crates/vesper-agent/src/vro/loop_detector.rs` (pure, no I/O,
`#![forbid(unsafe_code)]` inherited). `react.rs` constructs one guard at loop
entry and consults it at the OBSERVE step. Mirrors the VRO-10 placement
pattern of `rate_limit.rs` / `repair.rs`: a self-contained detector module
wired into exactly one loop.

---

## 3. State Machine Flow

Interception point: **post-observation** — after the `ToolInvoker` returns
and before the next THINK cycle, at the same site where
`trajectory.push(TrajectoryEntry::Observation { .. })` occurs
(react.rs:456–496 family).

```
            ┌──────────────── VRO ReAct loop (react.rs) ────────────────┐
            │                                                            │
   THINK ───┤ ReactAgent::next_action(prompt, &trajectory)               │
            │   └─ Finish ──────────────────────────────► Succeeded      │
            │   └─ CallTool { name, arguments }                          │
            │        │                                                   │
   ACT ─────┤   invoker.invoke(name, &arguments)                         │
            │        │  Read-Before-Write reject ──► synthetic obs       │
            │        │                      (NOT recorded, no budget)    │
            │        ▼                                                   │
   OBSERVE ─┤◄═ VRO-12 INTERCEPT ════════════════════════════════════════│
            │  guard.record(name, &args, &result)  [success only]        │
            │        │                                                   │
            │        ├─ Clear ───────► push Observation; continue        │
            │        ├─ Warn ────────► push Observation (real result),   │
            │        │                  push synthetic nudge Observation │
            │        ├─ Block ───────► push Observation (replaced text), │
            │        │                  NO max_tool_calls consumed       │
            │        └─ Break ───────► halt: BudgetExceeded + named risk │
            └────────────────────────────────────────────────────────────┘
```

Detectors run in fixed escalation order (most severe classification first),
evaluated over the 5-entry window after each successful record:

1. **Exact Repeat** — trailing run of identical `(name, args_hash)`.
   - run ≥ 3 ⇒ `Warn`
   - run ≥ 4 ⇒ `Block`
   - run ≥ 5 (window saturated) ⇒ `Break`
2. **Ping-Pong** — the entire window alternates `A, B, A, B, A` with
   `A ≠ B` (tool *names*; args may vary — a name-level pattern is the
   signal).
   - pattern present ⇒ `Warn`
   - pattern present AND a prior Warn for the same `(A, B)` pair is still
     inside the window ⇒ `Block`
   - pattern present AND an Exact-Repeat `Break` condition is also met ⇒
     `Break` (degrades to detector 1)
   - *Adaptation note:* the reference detects ping-pong on a 20-entry window
     at ≥ 4 full cycles (8 entries). A 5-entry window (C2, directive-fixed)
     can see at most 2 full cycles + 1 confirming entry, so cycle counting is
     replaced by whole-window pattern matching plus in-window escalation
     state. Thresholds are re-derived for the fixed window, not inherited.
3. **No-Progress** — ≥ 4 entries in the window share `(name, result_hash)`
   while exposing ≥ 2 distinct `args_hash` values (if all args are
   identical, detector 1 owns the case — the reference's separation, kept).
   Counted with `filter`, **not** `take_while`: an unrelated interleaved
   call must not reset the streak (the reference's 43-near-duplicate-calls
   lesson).
   - **Read-only tools only** (post-incident correction, see the decision
     log). The premise "byte-identical result ⇒ no new information" holds
     only for read-only probes: mutating/shell acks (`edited {path}`,
     `wrote N bytes`) are constant-form text that does not encode the state
     change, and an empty `grep` result is legitimate for every
     non-matching pattern. Feeding those into the key produced fatal
     false-positive `Break`s in real coding turns (the TUI incident of
     2026-W14: five legitimate differently-argued `edit_file` calls and
     five empty-result `grep` probes each killed a live turn). The recorded
     `ToolExecutionClass` gates the detector: only windows whose matching
     records are `ReadOnly` may classify No-Progress. Exact Repeat is not
     gated (an identical call is a loop regardless of class).
   - Advisory only: count = 4 ⇒ one `Warn` for the evidence window;
     further differently-argued probes preserve their real results and
     continue. Empty output from several searches is not proof of a loop.
     Only exact-repeat protection and the configured numeric tool budget may
     end a turn. This incident correction prevents both hosts from reporting
     a failed implementation during legitimate repository exploration.

Escalation state is a small in-window record of the last emitted
intervention (pattern kind + pair/.tool identity) — no cross-turn memory,
no growth: it is part of the fixed-size guard struct and resets when the
evidence pattern leaves the window.

`Break` maps to the loop's existing halt semantics: return
`ReasoningOutcome { status: BudgetExceeded, unresolved_risks:
["VRO-12 loop guard: <pattern>, <tool(s)>, <counts>"] }` — the same shape
`max_tool_calls` exhaustion produces (react.rs:434–436), so hosts need no
new handling. `run_tool_grounded_react_with_trajectory` still returns the
partial trajectory on `Break` (VRO-7 extraction keeps working).

---

## 4. Intervention Strategy

The nudge is a synthetic `TrajectoryEntry::Observation` appended
immediately after the real result, prefixed `[VRO-12 Loop Guard]`, and is
the **only** context mutation VRO-12 performs. It rides the exact channel
Read-Before-Write already uses, so every `ReactAgent` implementation
(including `LmStudioReactAgent`) sees it with zero adapter changes.

Warn text (per pattern):

- Exact Repeat: `[VRO-12 Loop Guard] You have called '{tool}' {n} times in
  a row with identical arguments and it returned the same result. Repeating
  it again will be blocked. State what the repeated results prove, then take
  a different action or Finish.`
- Ping-Pong: `[VRO-12 Loop Guard] '{A}' and '{B}' have alternated for the
  entire recent window without new state. Choose a different strategy,
  change the arguments substantively, or Finish with what you have.`
- No-Progress: `[VRO-12 Loop Guard] '{tool}' has returned byte-identical
  output across {n} differently-argued probes. The information source is
  exhausted. Stop probing it; reason from the observations already
  collected, or Finish.`

Exact-Repeat/Ping-Pong Block replaces the result text with
`[VRO-12 Loop Guard — BLOCKED] '{tool}' suppressed: {pattern}. {guidance}`
and — mirroring Read-Before-Write — the blocked attempt does **not** consume
a `max_tool_calls` unit (it never influenced the model's next decision with
new information). No-Progress never blocks or terminates; its one warning is
advisory. Break terminates with the named risk note (§3) only for the
remaining terminal patterns.

Rationale for the ladder: the reference's production telemetry shows the
Warn nudge alone breaks most loops (the model gets an explicit instruction
to change strategy); Block exists for the model that ignores the nudge;
Break exists for the pathological case where both fail. Escalation is
evidence-driven, never time-driven — Vesper has no wall-clock detector by
design (C5), unlike the reference's separate `min_elapsed_secs` gate which
we deliberately do not port (it exists there to protect long-running
browser workflows; Vesper's ReAct budgets are already wall-clock bounded by
VRO-9).

---

## 5. Non-Goals

- No detection inside non-ReAct orchestration strategies. The shared direct
  `agent_loop.rs` path is intentionally covered to enforce ACP↔TUI parity.
- No new `ReasoningConfig` / `vesper-domain` surface. Thresholds are
  module-local constants in `loop_guard.rs` (the `react.rs` precedent for
  loop-local policy constants).
- No cross-turn or cross-session loop memory; no persistence; no telemetry
  port (the `AgentProgressPort` already surfaces outcomes).
- No time-gated identical-output abort (reference's
  `check_identical_output_abort`): redundant with VRO-9 wall-clock ceilings
  and incompatible with C5 determinism.
- No fuzzy/similarity matching. Hash equality only — near-duplicate text is
  out of scope and honestly absent.

---

## 6. Verification

Canonical gate (must be run and green before merge):

1. `cargo test -p vesper-agent` — all existing **336** tests green, plus the
   new suite below.
2. `cargo test --workspace --all-features` — workspace floor **≥ 1193 test
   functions** (measured at HEAD; the directive's "1028+" is already
   exceeded — the gate is *no regression below the measured floor*, not the
   stale number).
3. `cargo xtask verify` — fmt + Clippy `-D warnings` + workspace tests +
   architecture scan (`vesper-agent` dependency direction must still pass
   with an empty `Cargo.toml` diff).
4. `git diff -- crates/vesper-agent/src/vro/react.rs` reviewed for the C4
   signature freeze.

Required new unit tests (deterministic fakes — the existing `react.rs`
scripted-agent/invoker pattern, cf. `budget()` helper and
`loop_halts_when_max_tool_calls_is_exhausted`). All are present; names map
to the labels above:

- **Window mechanics (W1)** — `window_retains_at_most_five_records_after_many_calls`
  (also pins that the retained five are the *newest* five).
- **Canonical hashing** — `args_hash_is_invariant_under_object_key_reordering`
  (H1); `hash_distinguishes_zero_float_and_string_zero` (H2);
  `hash_distinguishes_adjacent_string_concatenations` (H3).
- **Exact Repeat** — `exact_repeat_warns_at_three_identical_calls` (E1, Warn +
  nudge + loop continues); `exact_repeat_blocks_at_four_identical_calls` and
  `react_loop_block_does_not_consume_tool_budget` (E2, Block + replacement +
  no `max_tool_calls` consumption);
  `exact_repeat_breaks_when_window_saturates` +
  `react_loop_breaks_on_repeat_pattern` (E3, Break + `BudgetExceeded` + named
  risk); `with_trajectory_variant_returns_partial_trajectory_on_break` (E4).
- **Ping-Pong** — `ping_pong_warns_on_full_window_alternation` (P1);
  `ping_pong_negative_case_abbaa_is_clear` (P2);
  `ping_pong_blocks_when_pattern_persists_after_warn` (P3).
- **No-Progress** — `react_loop_no_progress_warns_without_stopping_exploration`
  and `no_progress_interleaved_call_does_not_reset_the_count` (N1/N2,
  `filter` semantics); `identical_args_stay_exact_repeat_not_no_progress`
  (N3). It is advisory-only; the numeric tool budget remains its only
  terminal ceiling.
- **Recording rules** — `failed_invocations_are_not_recorded_by_the_loop`
  (R1); `read_before_write_rejections_are_not_recorded_and_consume_no_budget`
  (R2, asserts `tool_calls == 0` by exact cost arithmetic, not token counts).
- **Zero-breakage gold** — Z1 is **not implementable** without inventing a
  disable switch (see decision D2); its testable core is
  `non_looping_react_turn_produces_no_loop_guard_text` (Z2): a non-looping
  successful ReAct turn (distinct tools/results) produces no `[VRO-12` bytes
  anywhere.

Soak/property additions (follow the `tests/soak_test.rs` `#[ignore]`
pattern): (S1) 200-call adversarial mixed workload ⇒ guard memory bounded
(window never exceeds 5; no growth), all classifications deterministic
across two identical runs.

---

## 6.5 Audit Outcomes and Binding Decisions (2026-08-29)

A full evidence-based audit of every §6 requirement against production code
and tests was performed after the initial IMPLEMENTED claim. Findings and
decisions below are binding; the audit evidence lives in
`docs/result-aware-loop-detection-audit-report.md`.

### Gaps found and closed

| Requirement | Finding | Resolution |
|---|---|---|
| W1 | No test recorded more than 5 calls, so the window bound was never exercised. | Added `window_retains_at_most_five_records_after_many_calls` (8 calls ⇒ `len() == 5`). |
| H1 | No canonical-JSON key-order test existed despite PRD §2.2 claiming "a guard test pins this". | Added `args_hash_is_invariant_under_object_key_reordering`. |
| H2 | No `0` / `0.0` / `"0"` distinction test existed. | Added `hash_distinguishes_zero_float_and_string_zero`. |
| H3 | No `["ab","c"]` vs `["a","bc"]` test existed. | Added `hash_distinguishes_adjacent_string_concatenations`. |
| E4 | `with_trajectory_variant_returns_partial_trajectory_on_break` absent; only the non-trajectory Break path was covered. | Added the test; asserts non-empty trajectory plus a real (Action, success Observation) pair. |
| P2 | The negative `A,B,B,A,A` case was never tested. | Added `ping_pong_negative_case_abbaa_is_clear`. |
| N2 | The interleaved-call `filter`-semantics claim had no test. | Added `no_progress_interleaved_call_does_not_reset_the_count`. |
| N3 | The "identical args stay Exact Repeat" separation had no test. | Added `identical_args_stay_exact_repeat_not_no_progress`. |
| R1 | Failure-exclusion was asserted nowhere. | Added `failed_invocations_are_not_recorded_by_the_loop` (turn halts on the numeric ceiling; zero VRO-12 bytes). |
| R2 | Read-Before-Write exclusion and its zero-budget property were untested. | Added `read_before_write_rejections_are_not_recorded_and_consume_no_budget`. |
| Z2 | The no-intervention gold case was untested. | Added `non_looping_react_turn_produces_no_loop_guard_text`. |
| S1 | The 200-call soak did not exist. | Added `soak_loop_detector_200_call_mixed_workload_bounded_and_deterministic`. |
| C3 | Only the Break tier of the direct `AgentLoop` path was tested; Warn/Block parity was unproven. | Added `direct_loop_surfaces_loop_guard_warning_without_failing_the_turn` and `direct_loop_blocks_fourth_identical_call_without_counting_it_as_success`. |

### String-prefix couplings removed (typed state)

Two load-bearing string-prefix comparisons existed between the loop-detector
intervention text and unrelated consumers. Both are replaced with typed
state:

1. `LoopGuardAction::Break(String)` → `LoopGuardAction::Break(LoopBreak)`,
   where `LoopBreak { pattern: LoopPattern, message: String }`. The direct
   `AgentLoop` test now classifies via the typed `LoopPattern` instead of
   `reason.contains("VRO-12")`, and both loops still surface
   `breakage.message` as the human-readable `unresolved_risks` /
   `AgentLoopError::LoopDetected` note.
2. The direct loop's success classification
   (`!output.starts_with("[SYSTEM OVERRIDE: LOOP BLOCKED")`) → a typed
   `blocked_by_loop_guard: bool` set only by the `LoopGuardAction::Block`
   arm. The guard's message wording can no longer silently decouple from
   success classification.

Note: the pre-existing `"tool error:"` / `"permission denied:"` /
`"unknown tool:"` prefixes in `agent_loop.rs` are the harness's own
`GateOutcome` text contract, produced and consumed inside the same module;
they are out of VRO-12 scope and were left alone.

### PRD-vs-implementation differences, resolved

- **Ping-pong thresholds.** §3 specifies Warn on the whole-window pattern,
  Block only after a prior in-window Warn for the same pair, and Break only
  by degradation to Exact Repeat. The implementation matches this exactly
  (see `ping_pong_warns_on_full_window_alternation`,
  `ping_pong_blocks_when_pattern_persists_after_warn`,
  `ping_pong_breaks_when_alternation_fills_every_slot`). The reference's
  20-entry/4-cycle thresholds are **not** portable to a directive-fixed
  5-entry window; §3's adaptation note already documents this and the
  implementation is the evidence-backed reading. No change.
- **No-progress escalation.** Live TUI and ACP evidence showed that several
  valid, differently-argued `grep` probes with equal empty output were
  reported as failed turns. Equal output is insufficient evidence that a
  repository survey is unsafe. No-progress therefore emits one in-window
  warning at four matching probes and never `Block`s or `Break`s; exact
  repeat and the numeric tool budget retain the terminal protections.
- **Intervention text.** §4 quotes bare `[VRO-12 Loop Guard]` strings. The
  implementation prefixes Warn with `[Loop Detection Warning]` and Block
  with `[SYSTEM OVERRIDE: LOOP BLOCKED. YOU MUST CHANGE STRATEGY.]`. These
  prefixes do **not** appear in the zeroclaw reference or in the frozen
  Python oracle (verified by grep in both). They are Vesper-local phrasing
  intended to make the escalation tier legible to the model; they are kept,
  and §4's quoted strings are understood to be the *body* following the
  tier prefix. No behavioural requirement depends on the exact prefix since
  the typed-state change above removed all prefix-based classification.
- **Z1 (disabled-guard byte-identical gold) is obsolete as written.** There
  is no disable switch, `ReasoningConfig` surface, or configuration seam for
  the detector — and §5 explicitly forbids adding one. The reference's
  `LoopDetectorConfig.enabled` exists because that repo derives it from a
  runtime `PacingConfig`; Vesper has no such seam and adding one solely to
  satisfy a test would violate §5 and the project's no-invented-surface
  rule. Z1's *intent* — zero behavior change on non-looping paths — is
  honored and proven by Z2 (`non_looping_react_turn_produces_no_loop_guard_text`):
  when no pattern fires the guard performs no context mutation, so the
  trajectory is identical by construction. Z1 is therefore retired in favor
  of Z2 and this decision is recorded here.

### Verification floor

Measured after this audit: **1235** workspace test functions
(**398** in `vesper-agent`), exceeding the §6 floor of 1193/336.
`cargo fmt`, `cargo clippy --workspace --all-targets --all-features
-D warnings`, `cargo xtask architecture`, and `cargo xtask verify` are all
green. `git diff` on `crates/vesper-agent/Cargo.toml` is empty (C1) and no
public `ReactAgent` / `ToolInvoker` / orchestrator signature changed (C4).

---

## 7. Evidence Appendix (recon, all paths verified this session)

**agent-vesper** (`/home/Alex/Projects/agent-vesper`, clean at HEAD):

- `crates/vesper-agent/src/vro/mod.rs:115` — `VroOrchestrator`; VRO-1..VRO-7
  entry points (`execute`, `execute_with_judge`, `execute_react`,
  `execute_with_critic_adjudicator`, `execute_with_learning`).
- `crates/vesper-agent/src/vro/react.rs` — the loop this PRD instruments:
  trajectory pushes at 456/460/477/486/496; halt semantics at 402–436;
  Read-Before-Write synthetic-observation precedent; test patterns at
  695–767.
- `crates/vesper-agent/src/vro/rate_limit.rs`, `repair.rs` — the VRO-10
  placement precedent (self-contained detector wired into one loop;
  `RepairController::is_repeated_attempt` is the existing single-shot
  signature-repeat check VRO-12 generalizes).
- `crates/vesper-agent/Cargo.toml` — `sha2.workspace = true` (VRO-7) ⇒ C1
  satisfiable with an empty diff.
- `crates/vesper-agent/AGENTS.md` — ownership + verification gates cited in
  §6.
- VRO numbering: ADR 0017 = VRO-11 ⇒ VRO-12 free.
- Test counts: `grep -c '#\[test\]|#\[tokio::test\]'`-equivalent shell
  census: 1193 workspace / 336 vesper-agent.

**zeroclaw** (`/home/Alex/Projects/zeroclaw` @ `d5b69f5`, read-only data):

- `crates/zeroclaw-runtime/src/agent/loop_detector.rs` (1002 lines) —
  `LoopDetectorConfig` (enabled/window 20/repeats 3), `ToolCallRecord`
  {name, args_hash, result_hash}, `VecDeque` window, detectors in
  escalation order, Warning→Block→Break ladder, canonical JSON walker with
  the `0` vs `0.0` `Number::hash` collision patch, `filter`-based
  no-progress counting.
- `crates/zeroclaw-runtime/src/agent/turn/mod.rs:524–528, 1373–1395` —
  construction + post-collection wiring.
- `crates/zeroclaw-runtime/src/agent/turn/results_collect.rs:159–230` —
  record-on-success-only, `loop_ignore_tools` exclusion, nudge injection
  via system message, Break ⇒ bail.
- `crates/zeroclaw-config/src/schema.rs:6017–6070` — shipped defaults
  (enabled=true, window=20, repeats=3) and the exact-repeat / ping-pong /
  no-progress taxonomy named verbatim.
