# Final audit: acceptance enrollment visibility & bounds

Date: 2026-09-15. Scope: the full 2026-09-14 work unit (`8d126bd`) plus
its PRD (`docs/acceptance-enrollment-visibility-prd.md`) and execution
report, re-traced against current code — not against the report that
claims it.

Trigger: Alex asked "did you do the final audit?" — the delivery had
shipped with red-first receipts per fix but without the audit pass
(sabotage battery, vacuity checks, hand re-derivation, narrative-vs-
receipt re-read).

## Method

1. Re-read the PRD and execution report end to end.
2. Re-traced every AC claim to current code with fresh greps/reads.
3. Sabotage battery in throwaway worktrees: revert each fix
   individually, confirm each new test fails.
4. Vacuity checks on each new test (does it encode the invariant, or the
   implementation's output?).
5. Hand re-derivation of the time-bound math.
6. Narrative-vs-receipt re-read.

## Findings

### F1 (material) — AC-3 was never tested; "all six ACs green" was false

The PRD outcome row claimed all six ACs green. The test file contained
exactly three new tests: AC-1 (visibility), AC-2 (ladder cap), AC-4
(refusal loudness/no-save). The third is commented "AC-4/AC-3" but
exercises refusal behavior — nothing measures the wall-clock ceiling.
The report's own test table omitted a ceiling row, and no grep found any
ceiling/sleep/Duration assertion in `acceptance_tests.rs`.

**Correction in place**: PRD §7 outcome annotated; §8 audit note added
with the exact deltas. Not silently rewritten.

### F2 (material) — the ceiling did not cover the whole enrollment path

The 300 s ceiling wrapped only the scope review
(`reviewer.inspect`). The subsequent `active.prepare()` call — the
contract ladder of up to two attempts, each a nested reviewer loop
individually timed out at 180 s — ran **outside** the window with no
elapsed re-check. Hand math: 180 + 2 × (180 + 180) ≈ **900 s ≈ 15–17
min**, not the documented ~5. The PRD's D2 ("the whole enrollment
execute() path carries a wall-clock ceiling of 300 s") was overstated.

**Runtime proof**: on a worktree at the pre-audit code with the
contract-ladder pin added, the test ran **1,208 s** and failed —
Alex's reported freeze symptom reproduced in-process at the exact phase
the report claimed was bounded.

### F3 (minor, corrected during audit) — narration drift

The report's red-first section described the runtime strip as "with all
progress emission stripped from the fixed tree"; the actual receipt was
the visibility test failing with `Status line missing: []` on a tree
where the bridge existed but emission was removed — the essence holds,
wording tightened during the audit re-read.

### F4 (hygiene) — dead closure introduced then removed

The first repair attempt left an unused `enrollment_window_elapsed`
closure (clippy caught it). Deleted; guards are inlined and commented.

## Repairs (both red-first)

| Repair | Red receipt (pre-repair code) | Green receipt (current) |
|---|---|---|
| Ceiling covers contract ladder | `enrollment_ceiling_covers_the_contract_ladder_too` ran **1,208 s**, FAILED | passes; window shared via `enrollment_ceiling()` |
| AC-3 test exists | `enrollment_wall_clock_ceiling_bounds_the_total_window` **hung the full 600 s** (pre-fix tree; the freeze reproduced) | passes in 10 s via `#[cfg(test)]` override; production stays 300 s |

The test override is `#[cfg(test)]`-gated (zero production effect) and
defaults to the production 300 s value; CI cost is two ~10 s tests.

## Sabotage battery (post-repair)

- Reverting the ladder cap 2→3 ⇒ `contract_review_ladder_is_capped_at_two_attempts` FAILED (`left: 3, right: 2`). ✅
- Reverting the ceiling ⇒ the ceiling pin **hung** (600 s runner timeout). ✅ (The hang *is* the pre-fix behavior; the pin detects it as failure.)
- Pre-fix tree (no bridge at all) ⇒ visibility test absent there; AC-1's runtime red receipt stands from the fix session (emission stripped ⇒ `Status line missing: []`).

## Re-derived invariants

- **One shared window**: `enrollment_ceiling()` is computed once per
  enrollment call; every phase guard reads the same
  `enrollment_started`. No phase can run on a fresh budget.
- **Fail-loudly, save nothing**: all three terminal outcomes (scope
  refusal, window expiry, contract-ladder exhaustion) return
  `ToolError::Failed` with "do not retry / ask the user" text, leave
  `enrolled` unset, and never save settings (pinned by four no-save
  assertions across the suite).
- **Scope untouched**: review prompts, verdict rules, receipt validation
  and ADR 0028 semantics unchanged; `cargo xtask acceptance` still 23/23.
- **Host parity**: `AgentProgressEvent::Status` renders live in both
  hosts (TUI `push_activity("◌ {text}")` at `main.rs:8569`; ACP
  `ReasoningDelta` at `acp/lib.rs:2039`) — verified in code, and both
  hosts carry a factory wired with a progress port
  (`acceptance_factory` in the ACP prompt path; `SessionStatusPort` in
  the TUI).

## What was checked and held

- AC-1/2/4 tests have teeth (sabotage-verified above).
- Ladder bound: exactly 2 attempts (pinned by an atomic counter).
- No-save pins across refusal/cancellation/missing-file paths — green.
- Workspace floor: **2,331 passed / 0 failed** (2,329 + 2 audit pins).
- `cargo xtask acceptance` 23/23 · clippy clean under the exact CI flags
  (`--workspace --all-targets --all-features -- -D warnings`) · fmt
  clean · naming-guard 18 frozen (0 erosion).
- Cross-platform CI: **4/4 workflows success on `8e13c40`**
  (pull-request-validation, msrv, five-target-foundation, web-driver).

## Fix iteration honesty (continuation, 2026-09-15)

The audit's own repair initially repeated the verification mistake it
was auditing for:

- **CI caught clippy defects in the audit-repair code itself**
  (`bc0ddf1` → failed `pull-request-validation`): a useless `format!`
  on the scope-review guard and two `to_string`-in-format-args lints in
  the audit pins. Root cause: the local clippy run omitted
  `-D warnings` and the log-grep filters swallowed exactly those lint
  classes. Fixed in `c41b600`; local verification now runs the CI flags
  verbatim, unfiltered.
- **A same-day RustSec advisory** (RUSTSEC-2026-0285, rustls TLS 1.3
  handshake across encryption levels, medium, fix ≥0.23.45) fail-closed
  the supply-chain advisory gate. Minimal lockfile bump
  `rustls 0.23.42 → 0.23.45` (`8e13c40`); workspace 2,331/0 and
  acceptance 23/23 re-run after the bump.

All receipts above were taken on the final tree (`8e13c40`), not the
intermediate ones.

## Verdict

The 2026-09-14 delivery was **materially incomplete against its own
PRD**: one AC untested, one bound weaker than documented. Both repaired
red-first with runtime receipts. The initiative's user-facing claim —
"enrollment can no longer look like a freeze" — now holds at the
documented ~5-minute worst case, with the mechanism pinned.

## Open items

- Release: the repaired code is on `main`, pushed and CI-green on
  `8e13c40`, but unreleased; ships with
  the next version cut.
- Real-device acceptance of the repaired UX remains Alex's
  (update → enable acceptance → confirm status lines within seconds and
  a ≤5-minute visible bound).
