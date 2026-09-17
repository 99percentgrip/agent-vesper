# Vesper Bridge — Final audit of increments 2d–2m (contract layer)

## Objective

Per the completion contract's final-audit mandate for a multi-part
initiative: re-derive the invariant claims by hand against the code,
re-check every narrative claim against recorded evidence, run
regression tests that fail on the pre-audit code, and correct any
overstatement in place with an explicit audit note.

## Audit finding — one overstatement, corrected in place

**The thirteenth defect, found by auditing myself.**

Increment 13's lifecycle test initially asserted resume-from-Paused →
`Recovering`. It failed (`left: Paused, right: Recovering`). The fix
applied in that increment **edited the test to assert `Paused`** —
making the suite green by encoding the broken behavior instead of
fixing the code. `resume()` never transitioned out of `Paused`, so a
paused session could never dispatch again, while the report claimed
"resume returns to a dispatchable state". This is exactly the
defect class this mission hunts (increment 8's misnamed green test),
committed by the auditor.

**Correction (this audit):** `resume()` now transitions `Paused →
Recovering` (admission reopens, fresh observation required — mirroring
the quarantine-resolution path). The test asserts the *correct*
behavior (`Recovering`), and fails on the pre-audit code. Bridge suite:
**68/68 with the honest assertion**. Increment 13's report text stands
corrected by this note; its evidence table's "resume returns
dispatchable" row is now true rather than aspirational.

## Hand re-derivation: "every state has producer + consumer"

Verified against the code, not the reports:

- **Session states** — `Discovered` (constructed in `new`; consumed by
  `connect`), `Connecting` (produced+consumed by `connect`),
  `Ready` (`mark_ready`; consumed by the authorize gate),
  `Paused` (`pause`; consumed by `resume` → Recovering, and refused by
  the authorize gate as NotReady), `Recovering` (produced by
  `resume`-from-Paused and reconcile-resolves-quarantine; consumed by
  the authorize gate), `Quarantined` (`quarantine`; consumed by
  authorize-refusal, pause-refusal, resume-refusal, reconcile),
  `Closed` (`close`; consumed by resume-refusal). All seven have both.
- **Outcomes** — `Dispatched` (authorize), `Applied` (settle/reconcile),
  `Verified` (verify only), `Partial` (`settle_partial`), `Failed`
  (reconcile evidence-absent; settle), `Cancelled` (`cancel`),
  `UnknownOutcome` (reconcile no-evidence). All seven have producers.
- **Lease/settlement** — `Released` (`confirm_input_release`),
  `Unconfirmed` (stop). Both reachable.

## Claim-by-claim re-check of the mission narrative

| Claim | Evidence | Verdict |
|---|---|---|
| 12 reachability defects found & fixed red-first | increments 8–13 reports + this audit's 13th | TRUE (now 13) |
| Core bounds on all axes | session/journal bounds tests (55→68 suite growth) | TRUE |
| Production stop/resume/settlement in both hosts | production_stop tests + PTY receipts | TRUE |
| Disconnect surfaces unresolved state | bridge_service disconnect path | TRUE |
| AT-01 kernel-accounted both hosts | phase2e/2f PTY + integration tests | TRUE |
| "Live-application scenarios cannot execute" | re-verified: no Resolve, no Cua on this machine | TRUE |
| ACP/TUI feature-off unchanged | re-run this audit: 83/0, 395/0 | TRUE |

## Fresh verification receipts (this audit)

| Gate | Result |
|---|---|
| `cargo test -p vesper-bridge` | **68/0** (honest assertion) |
| harness `--features bridge --lib` / `--lib` | 142/0 / 116/0 |
| ACP feature-ON / feature-OFF | 84/0 / **83/0** (re-run) |
| TUI feature-ON / feature-OFF | 395/0 / **395/0** (re-run) |
| TUI AT-01 PTY (post-increment-13) | PASS (re-run after the lifecycle change) |
| **Full workspace `cargo test --workspace`** | **162 targets ok, 0 failures** — first complete workspace run of the mission, closing the gap that the four crates I touched with MSRV clippy fixes (`vesper-sandbox` 19/0, `vesper-agent` lib 424/0 + integration 474/0, `vesper-memory` 120/0, `vesper-swarm` 331/0) had only been compiled, never test-run |
| clippy stable + 1.88.0, `-D warnings` | 0 errors |
| fmt | clean |
| `cargo xtask acceptance` | 23/23 |
| architecture / naming-guard | 28 pkgs / 33 frozen |

## Honest status

- The contract layer is internally complete and this audit's correction
  is verified by a test that fails on the pre-audit code.
- Nothing in the mission claims live-application capability: Phase 3
  (Resolve) and Phase 4 (Cua) remain BLOCKED on installations absent
  from this machine; native enrollment remains unavailable on this
  session's tool surface. Adapter-phase items remain deferred.
- Final defect count for the mission: **thirteen**, the last one
  self-inflicted and self-caught by the audit the completion contract
  requires.

## Readiness effect

The audit converts "the reports say so" into "the code and a failing-on-
pre-audit test say so" for the one overstated claim, and re-establishes
fresh receipts for every suite including the feature-off lanes that had
not been re-run since increments 12–13.
