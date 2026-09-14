# PRD: Acceptance enrollment visibility & bounds

| | |
|---|---|
| **Status** | COMPLETE — 2026-09-14 · **audited & repaired 2026-09-15**: AC-3 had no test and the ceiling did not cover the contract ladder (true worst case ~17 min, not ~5). Both repaired red-first; see [audit note](#9-audit-correction-2026-09-15) |
| **Target** | `vesper-harness/src/acceptance.rs` (+ host wiring, tests) |
| **Owner** | Alex (product) |
| **Related** | `native-acceptance-completion-prd.md` (the gate), ADR 0028 (completion assurance), `voice-f5-cancel-trap-repair.md` (same symptom class: silent long work looks frozen) |

## 1. Problem (verified on the current tree)

With acceptance enabled (Settings ON or `/acceptance start <PRD>`), the
next implementation turn can appear to freeze:

1. **Silent nested reviewers.** `acceptance_enroll` runs a chain of
   nested reviewer agents (scope review → up to 3 contract attempts ×
   contract+coverage reviews), each a full provider loop with a 180 s
   timeout (`acceptance.rs:318-330`). Their only progress port is an
   internal `read_file` counter (`acceptance.rs:273-286`); **no event
   reaches the host UI**. The outer turn emits exactly one
   `ToolStarted("acceptance_enroll")` and then silence for many minutes.
2. **No enrollment bound.** Nothing caps total enrollment duration or
   reviewer attempts beyond the per-review 180 s timeout; worst case is
   ~7 sequential nested runs (20+ minutes) inside one tool call.
3. **Dead-end grind.** In automatic mode with no model-callable PRD path
   (or a refused enrollment), each failed `acceptance_enroll` returns a
   tool error, the model retries or tries to finish, completion is
   refused (`agent_loop.rs:1156`), and the turn grinds toward the
   iteration ceiling (up to `max_tool_iterations ×
   MAX_PLAN_CONTINUATION_SEGMENTS`, default effectively 4000) — silent
   the whole way.

## 2. Goals

- **G1 — Visibility.** Nested reviewer activity streams to the host
  progress port as first-class events, so hosts render live status.
- **G2 — Bounded enrollment.** Total enrollment review time is bounded
  (one scope-review pass + one contract pass, each still individually
  timed out); no unbounded attempt ladders inside one tool call.
- **G3 — Loud failure.** If enrollment fails or is refused, the gate
  surfaces one actionable host-visible outcome and stops retrying inside
  the turn: the model is told to ask the user, not to grind.
- **G4 — No scope weakening.** Evidence rules, review independence,
  receipt rules and ADR 0028 behavior are untouched: reviews still gate,
  refusals still refuse, nothing auto-completes.

## 3. Non-goals

- Changing review prompts, verdict rules, or receipt validation.
- Making enrollment optional or auto-skipping in automatic mode.
- ACP-side UX changes beyond the shared progress-event enum.

## 4. Decisions

- **D1 — Progress bridge.** `AgentProgressEvent::Status { text }` (new
  variant, host-rendered as an activity line) is added to the shared
  enum; both hosts render it (TUI activity line, ACP reasoning-delta
  line). The nested reviewer forwards a bounded stage line at each
  reviewer start (never tool payloads), via a progress port passed
  through `WorkerFactory`-constructed reviewers. The internal
  `read_file` counting port remains (composes with the bridge).
- **D2 — Enrollment structure.** Scope review (enroll) keeps its single
  call; the contract pass is capped at **one** attempt ladder of at most
  two proposals (was 3) and the whole enrollment execute() path carries
  a wall-clock ceiling of **300 s** — exceeded → loud failure per D3.
  (Both hosts' existing 180 s per-review timeout still applies inside.)
- **D3 — Loud bounded outcome.** On enrollment refusal/failure/timeout:
  emit `Status`("acceptance enrollment failed: <reason> — ask the user
  for the correct PRD path"), save nothing, keep the gate unenrolled,
  and return a tool error whose text instructs the model to stop retrying
  and ask the user. The existing per-attempt ladder is removed.
- **D4 — Tool-call attribution.** Hosts already render
  `ToolStarted/Finished` for `acceptance_enroll`; the new `Status` events
  nest under it naturally in both UIs.

## 5. Acceptance criteria

- AC-1 (red-first): a progress-recording harness asserts `Status` events
  arrive while `acceptance_enroll` runs; fails on the pre-fix tree.
- AC-2: enrollment ladder capped (a refusing contract reviewer is called
  at most twice, then a loud bounded failure — no third attempt).
- AC-3: enrollment wall-clock ceiling enforced (stub reviewer that sleeps
  past the ceiling yields the loud bounded failure, not a hang).
- AC-4: refusal path emits the actionable `Status` line and does not save
  settings (existing no-save pins keep passing).
- AC-5: hosts compile and render: TUI shows the status line in activity;
  ACP forwards it as a reasoning-delta line. Both match exhaustively.
- AC-6: all pre-existing acceptance pins (no-save, forged-scope refusal,
  restart-fresh-evidence, mutation gate targets) stay green; workspace,
  acceptance 23/23, clippy, fmt, naming-guard clean.

## 6. Risks

- A slow real model may need the full ceiling; the ceiling turns a silent
  20-minute hang into a visible ≤5-minute bounded failure with a clear
  next step. Acceptance evidence rules unchanged.
- New enum variant is exhaustive-match breaking by design; both hosts are
  updated in the same change (cross-host parity rule).

## 7. Outcome (2026-09-14)

All six ACs green. Receipts:
`foundation/acceptance-enrollment-visibility-execution.md`.

## 8. Audit correction (2026-09-15)

The 2026-09-14 "all six ACs green" outcome was **overstated**:

- **AC-3 had no test.** Only AC-1/2/4 had tests (the file's third new
  test is labeled "AC-4/AC-3" but exercises refusal, not the time bound).
- **The ceiling did not cover the whole enrollment path.** It wrapped the
  scope review only; `prepare()`'s contract ladder (2 × ~180 s nested
  reviews) ran outside it. True worst case ≈ **17 minutes**, not the
  documented ≤5.

**Repair (red-first, both pinned now):**

- The 300 s window now guards **before and around** every phase,
  including the contract ladder (`enrollment_ceiling()` shared window).
- `enrollment_wall_clock_ceiling_bounds_the_total_window` — hung the
  full 600 s on the pre-fix tree (the reported freeze, reproduced
  in-process); passes in 10 s (test override) with the fix.
- `enrollment_ceiling_covers_the_contract_ladder_too` — ran
  **1,208 s** and failed on the pre-audit code; passes with the fix.
- Production ceiling unchanged at 300 s; the test override is
  `#[cfg(test)]`-gated and defaults to production behavior.
