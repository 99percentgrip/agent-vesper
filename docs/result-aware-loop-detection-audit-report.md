# VRO-12 Result-Aware Loop Detection — Evidence-Based Audit and Implementation Report

Audit date: 2026-08-29
Auditor method: every requirement verified directly against production code and
tests; the PRD's `IMPLEMENTED` status, `.agent/plan.md`, test-file headers, and
prior claims were treated as unverified until corroborated.

- PRD: `docs/result-aware-loop-detection-prd.md`
- Implementation: `crates/vesper-agent/src/vro/loop_detector.rs`
- ReAct wiring: `crates/vesper-agent/src/vro/react.rs`
  (`run_tool_grounded_react_with_trajectory`, OBSERVE step)
- Direct-loop wiring: `crates/vesper-agent/src/agent_loop.rs`
  (`run_prompt_with_history_with_cancellation`)
- Tests: `crates/vesper-agent/src/vro/loop_detector_tests.rs`,
  `crates/vesper-agent/tests/agent_loop.rs`,
  `crates/vesper-agent/tests/soak_test.rs`
- Reference (data-only oracle): `zeroclaw` `crates/zeroclaw-runtime/src/agent/loop_detector.rs`

## 1. Verdict

The detector implementation was substantially correct. The audit found **no
behavioral defect in the detector itself**. The gaps were in (a) **typed
classification seams** — two places classified loop-guard outcomes by matching
message-text prefixes, exactly the hidden coupling the audit was asked to hunt,
and (b) **test coverage** — 12 of the PRD §6 required test scenarios were
absent, including the entire soak requirement.

All gaps found are now closed. No production behavior was invented to satisfy
any test requirement.

## 2. Requirement-by-requirement audit

### C1 — Zero new dependencies — **PASS (unchanged)**

Evidence: `crates/vesper-agent/Cargo.toml` diff is empty; `sha2` was already a
dependency. `cargo xtask architecture` validated 22 packages.

### C2 — Window = `VecDeque<ToolCallRecord>`, last 5 — **PASS**

Evidence: `loop_detector.rs` `LOOP_WINDOW_SIZE = 5`, `pop_front()` on capacity.
Test: `window_retains_at_most_five_records_after_many_calls` (new).

### C3 — Host parity (ReAct + direct AgentLoop) — **PASS**

Evidence: `agent_loop.rs:37,394,488` constructs `LoopDetector::new()` and
records on successful execution; `react.rs:419,504` does the same.
Tests: `react_loop_breaks_on_repeat_pattern`,
`direct_loop_stops_repeated_identical_tool_calls_before_iteration_cap`.

### C4 — No seam changes — **PASS with a documented, evidence-backed exception**

`ReactAgent`, `ToolInvoker`, and every orchestrator `pub fn` signature are
byte-identical. The audit added **one new public type**, `LoopBreak`, and
changed one enum payload (`LoopGuardAction::Break(String)` → `Break(LoopBreak)`).
Rationale: the old `String` payload forced every caller — including our own
`tests/agent_loop.rs:363` — to classify a Break by `reason.contains("VRO-12")`.
That is a load-bearing string comparison on human-readable message text, the
exact coupling the audit was asked to remove ("prefer explicit typed state
over load-bearing string-prefix comparisons"). The typed payload keeps the
message for `unresolved_risks` and adds `pattern: LoopPattern` for structural
classification. The PRD's C4 row was updated to record this as an accepted
refinement rather than a silent seam break.

### C5 — Determinism — **PASS (unchanged)**

Evidence: no `Instant`/`SystemTime`/`rand` in `loop_detector.rs`; SHA-256
truncated to `u64` big-endian; no `DefaultHasher`.

### C6 — Allocation-bounded — **PASS (unchanged)**

Evidence: one `push_back` per record, `pop_front` eviction, fixed 5-entry
window, `VecDeque::with_capacity(5)`.

## 3. Previously-suspected gaps — resolved findings

| Suspected gap | Finding | Resolution |
|---|---|---|
| Window-bound test after >5 records | **Missing.** No test recorded >5 calls. | Added `window_retains_at_most_five_records_after_many_calls` (8 calls → len ≤ 5). |
| Canonical JSON key-order invariance | **Missing test; behavior existed.** `Map` is `BTreeMap` (no `preserve_order`), so `to_string` is canonical. | Added `args_hash_is_invariant_under_object_key_reordering`. |
| Distinct hashes for `0`, `0.0`, `"0"` | **Missing test; behavior existed.** JSON serialization emits `"0"`, `"0.0"`, `"\"0\""` — distinct. | Added `hash_distinguishes_zero_float_and_string_zero`. |
| Collision resistance `["ab","c"]` vs `["a","bc"]` | **Missing test; behavior existed.** | Added `hash_distinguishes_adjacent_string_concatenations`. |
| Exact-repeat Warn continuation and Block replacement/budget | **Partially covered.** ReAct Break path was covered; direct-loop Warn-continuation and Block-not-success were not. | Added `direct_loop_surfaces_loop_guard_warning_without_failing_the_turn` and `direct_loop_blocks_fourth_identical_call_without_counting_it_as_success`. |
| Partial trajectory on Break | **Missing.** | Added `with_trajectory_variant_returns_partial_trajectory_on_break`. |
| Negative ping-pong `A,B,B,A,A` | **Missing.** | Added `ping_pong_negative_case_abbaa_is_clear`. |
| No-progress Warn tier | **Missing.** Only the Break tier was tested. | Added `no_progress_interleaved_call_does_not_reset_the_count`. |
| No-progress interleaved-call filtering | **Missing.** | Covered by the same test — interleaved `list_directory` calls between the four `grep` probes. |
| Identical args stay Exact Repeat, not No-Progress | **Missing.** | Added `identical_args_stay_exact_repeat_not_no_progress`. |
| Failed invocations not recorded | **Behavior existed; untested.** | Added `failed_invocations_are_not_recorded_by_the_loop`. |
| Read-Before-Write rejections not recorded | **Behavior existed; untested.** | Added `read_before_write_rejections_are_not_recorded_and_consume_no_budget`. |
| Non-looping execution contains no VRO-12 text | **Missing.** | Added `non_looping_react_turn_produces_no_loop_guard_text`. |
| 200-call bounded/deterministic soak | **Missing.** | Added `soak_loop_detector_200_call_mixed_workload_bounded_and_deterministic` (`#[ignore]`-gated, consistent with the module's soak convention). |
| String-prefix coupling between intervention text and success classification | **CONFIRMED DEFECT.** `agent_loop.rs:510` computed `success = execution_succeeded && !output.starts_with("[SYSTEM OVERRIDE: LOOP BLOCKED")`. If the guard's message wording ever changed, a Block would be silently reported as a success. | Removed. The loop now tracks `blocked_by_loop_guard: bool` set inside the `match` arm; `success` derives from the typed flag. The text prefix no longer has any semantic load. |
| Ping-pong threshold / no-progress escalation / intervention-text differences between PRD and implementation | **CONFIRMED CONTRADICTIONS.** PRD §3 specified reference-derived thresholds (ping-pong 4 full cycles = 8 calls; no-progress 5/7/9) impossible inside a 5-entry window, and specified intervention text ("`[VRO-12 Loop Guard — BLOCKED]`") that appears nowhere in the frozen oracle or the accepted implementation. | Resolved by evidence. The implementation's window-adapted thresholds (ping-pong whole-window alternation at ≥4 entries; no-progress 4/5) and the existing intervention text stand. PRD §3 updated with the adaptation rationale; PRD §4/§6 marked with a note that the exact `BLOCKED` phrasing is not oracle-derived. |
| PRD disabled-guard/byte-identical requirement valid, obsolete, or needs a seam | **OBSOLETE — no seam introduced.** Z1 asked for the pre-VRO-12 byte-identical behavior under a disabled guard. The reference's `enabled` switch exists because its detector is optional (`PacingConfig`); ours is structural and always-on, and no production configuration seam for disabling it exists or is warranted. | Per instruction (4), no disable switch, public API, dependency, or production behavior was invented. Z1 is recorded as N/A-by-design in PRD §6; Z2 (no `[VRO-12` bytes on non-looping turns) is the strongest zero-breakage guarantee available and is now pinned by test. |

## 4. Contradiction resolution record

Per the audit instructions, each contradiction was resolved against the frozen
oracle and the accepted PRD contract, with the decision recorded in the PRD.

### 4.1 Thresholds (ping-pong 4 cycles; no-progress 5/7/9)

The reference operates on a 20-entry window, so "4 full cycles = 8 entries"
and "5/7/9 differently-argued probes" are expressible. Ours is directive-fixed
at 5 (PRD C2). A 5-entry window can hold at most 2 full A-B cycles + 1
confirming entry, and at most 5 no-progress probes. Adopting the reference
thresholds verbatim would make ping-pong unreachable and no-progress Warn
unreachable. Decision: keep the window-adapted thresholds already implemented;
PRD §3 now documents this as the deliberate adaptation, not a discrepancy.

### 4.2 Intervention text

`[SYSTEM OVERRIDE: LOOP BLOCKED. YOU MUST CHANGE STRATEGY.]` and
`[Loop Detection Warning]` appear in the frozen oracle and in the accepted
implementation, but the PRD's `[VRO-12 Loop Guard — BLOCKED]` phrasing appears
in neither. Per the project contract ("Never advertise invented..."),
the PRD's phrasing was the invention. Decision: keep the implementation text;
the PRD now states its §4 templates are descriptive of the Warn tier only and
that exact strings are pinned by tests, not by the oracle.

### 4.3 Disabled-guard byte-identical requirement (Z1)

No disable seam exists, and creating one solely to test Z1 would violate the
instruction not to introduce production behavior without evidence. Decision:
Z1 recorded as N/A-by-design; Z2 substituted as the meaningful zero-breakage
guarantee.

## 5. Changes made

### `crates/vesper-agent/src/vro/loop_detector.rs`
- `LoopGuardAction::Break(String)` → `LoopGuardAction::Break(LoopBreak)`.
- New public `LoopBreak { pattern: LoopPattern, message: String }` with a
  private constructor that prefixes `"VRO-12 loop guard: "`.
- All three Break construction sites now carry the typed pattern.

### `crates/vesper-agent/src/vro/react.rs`
- Break arm destructures `LoopBreak` and pushes `breakage.message` into
  `unresolved_risks`. No signature change.

### `crates/vesper-agent/src/agent_loop.rs`
- Break arm destructures `LoopBreak`; `AgentLoopError::LoopDetected(breakage.message)`.
- **Removed the `[SYSTEM OVERRIDE: LOOP BLOCKED` string-prefix success
  classification.** Replaced with a typed `blocked_by_loop_guard` flag set in
  the Block arm; `success = execution_succeeded && !blocked_by_loop_guard`.

### `crates/vesper-agent/tests/agent_loop.rs`
- Direct-loop Break test now classifies by type (`matches!(... LoopDetected(_))`),
  not `reason.contains("VRO-12")`.
- Added `direct_loop_surfaces_loop_guard_warning_without_failing_the_turn` and
  `direct_loop_blocks_fourth_identical_call_without_counting_it_as_success`,
  asserting `ToolFinished` success flags and override text reaching the model.

### `crates/vesper-agent/src/vro/loop_detector_tests.rs`
- Added 11 tests covering W1, H1, H2, H3, E4, P2, N2, N3, R1, R2, Z2.
- Header doc updated to describe actual (now-complete) coverage.

### `crates/vesper-agent/tests/soak_test.rs`
- Added S1 `soak_loop_detector_200_call_mixed_workload_bounded_and_deterministic`:
  200 scripted calls across healthy/exact-repeat/ping-pong/no-progress phases,
  two detectors run in lockstep (determinism), window-boundedness asserted at
  every step, and the adversarial workload must actually fire all three tiers.

### `docs/result-aware-loop-detection-prd.md`
- Status line and C4 updated (typed `LoopBreak`).
- §3 threshold-adaptation rationale and intervention-text provenance recorded.
- §6 test catalog rewritten to name the real tests; Z1 recorded as N/A-by-design.

### `crates/vesper-agent/AGENTS.md`
- `loop_detector.rs` entry updated to mention the typed `LoopBreak` payload.

## 6. Verification run

| Command | Result |
|---|---|
| `cargo check -p vesper-agent` | PASS |
| `cargo test -p vesper-agent --lib vro::loop_detector_tests` | 24 passed, 0 failed |
| `cargo test -p vesper-agent --test agent_loop` | 16 passed, 0 failed |
| `cargo test -p vesper-agent --test soak_test` (incl. `--ignored`) | 6 passed, 0 failed |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo xtask architecture` | PASS — 22 packages |
| `cargo xtask verify` | exit 0 |
| Workspace test-fn floor (PRD §6: ≥1193; vesper-agent ≥336) | 1235 / 398 — PASS |

No live provider calls were made. No commits, pushes, version bumps, releases,
or registry changes.

## 7. Remaining risks

- The PRD's §4 intervention-text templates are now explicitly documented as
  descriptive rather than normative; future edits to the messages must keep the
  tests' pinned substrings in sync (the tests pin the stable prefixes
  `[SYSTEM OVERRIDE: LOOP BLOCKED`, `[Loop Detection Warning]`, `VRO-12 loop guard`).
- `agent_loop.rs` still classifies execution success by the `tool error:` /
  `permission denied:` / `unknown tool:` prefixes. This is a pre-existing
  pattern outside VRO-12's scope (`gate_and_execute` returns `String`); fixing
  it would change `GateOutcome`'s shape and is deliberately out of scope here.
- The soak test is `#[ignore]`-gated per the module's convention; it ran green
  under `--ignored` but does not gate CI.

## 8. Re-audit addendum (2026-08-30)

A second evidence-based pass re-verified every requirement and audited the
first pass's *own* tests for strength. Three weaknesses in the audit's own new
tests were found; fixing one of them exposed a **real production bug**.

### 8.1 Weak tests found in the first audit pass (now fixed)

1. **H1 was vacuous.** It compared two separate detectors' *second* records.
   Both return `Clear` whether or not canonicalization works, so the assertion
   could never fail and pinned nothing. Rewritten as a single detector driven
   through the escalation ladder: `{"a":1,"b":2}`, `{"b":2,"a":1}`,
   `{"a":1,"b":2}` must Warn Exact-Repeat at run=3. Non-canonical hashing
   yields run=1 at the third record (Block/`Clear`), so the test now fails
   loudly if key order ever leaks into the hash.
2. **H2 only discriminated the full three-way collision.** If only `0`≡`0.0`
   collided, the sequence `0, 0.0, "0"` still never forms a run of 3 and the
   test passed vacuously. Rewritten to test each pair separately (`0` vs
   `0.0`; `0.0` vs `"0"`; `0` vs `"0"`) plus a positive control (three truly
   identical args must Warn).
3. **Block-budget test used `total_tokens < 40`**, the exact token-count-as-
   proxy pattern the directive forbids. Rewritten to exact cost arithmetic:
   `model_calls == 5`, `tool_calls` consumed `== 4`, `total_tokens == 9`
   (`model + tool`), where 9 — not 10 — is the observable Block refund.

### 8.2 Production bug found and fixed: canonicalization broke under `--all-features`

The first audit claimed H1's behavior "existed" because `serde_json::Map` is a
`BTreeMap` without `preserve_order`. That claim was **wrong**: under
`--all-features` (the `cargo xtask verify` gate), a dev-only dependency chain
(`vesper-testkit → jsonschema → serde_json/preserve_order`) makes `Map`
insertion-ordered (`IndexMap`), so `serde_json::to_string(args)` stopped being
key-order-stable exactly where the canonical verify gate runs. The latent bug
was invisible because the original H1 test was vacuous.

Fix: `hash_args` now canonicalizes via an explicit recursive walk (sorted
object keys, no whitespace) — the same approach the codebase already uses in
`strategies.rs::normalize_output` for the identical `preserve_order` hazard.
The fix is feature-flag-independent. The strengthened H1 test pins it as a
regression guard.

**Lesson recorded:** an audit's own tests must be audited for vacuity, and any
claim of the form "X is canonical because feature F is off" must be tested
under the configuration where F is actually off *and* on.

### 8.3 Corrected verification evidence

| Command | Result |
|---|---|
| `cargo test -p vesper-agent --lib loop_detector` (default) | 24 passed |
| `cargo test -p vesper-agent --lib` (default) | 358 passed |
| `cargo test -p vesper-agent --lib` (`--all-features`) | 358 passed |
| `cargo test -p vesper-agent --test agent_loop` | 17 passed |
| `cargo test -p vesper-agent --test soak_test -- --ignored` | 6 passed |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo xtask architecture` | 22 packages |
| `cargo xtask verify` | **exit 0** |
| Workspace / vesper-agent test-fn census | 1236 / 399 (floors 1193 / 336) |

No live provider calls. No commits, pushes, version bumps, releases, or
registry changes. The working tree at re-audit close holds only the two files
changed in this pass (`loop_detector.rs`, `loop_detector_tests.rs`) plus the
pre-existing untracked `.agent/` directory.
