# Vesper Bridge — Increment 11: the quarantine lifecycle is honest and resolvable (BR-18)

## Objective

Continuing the lifecycle-reachability sweep after increment 10's
unreachable settlement state: the **quarantine** lifecycle was broken in
two independent ways.

## The defects

1. **`resume()` lied about quarantined sessions.** A `Quarantined`
   session resumed with `Ok(())`, and the production `/bridge resume`
   path then reported "admission open; fresh observation required" —
   while `authorize()` refused every dispatch with `Quarantined`.
   A truthful-reporting lie: the host verb claimed recovery that the
   core could not deliver.
2. **Quarantine had no exit.** `reconcile()` quarantined on missing
   evidence ("until a human/reconciler resolves it", per its own doc),
   but **no code path anywhere cleared `Quarantined`.** One uncertain
   outcome permanently bricked the session — with resume also refusing
   (correctly per BR-18 semantics), the only path was disconnect.

## Methods and commands

- Red-first `crates/vesper-bridge/tests/quarantine_lifecycle.rs`:
  1. resume on a quarantined session must refuse AND leave the state
     quarantined;
  2. evidence-backed reconciliation must move the session to
     `Recovering` (the uncertainty is resolved; resume then works).
  Both failed on the pre-repair code (resume returned `Ok`, state stayed
  `Quarantined` after reconcile-with-evidence).
- Core repairs in `session.rs`:
  - `resume()`: `Quarantined` refuses with `CleanupUnconfirmed`
    (quarantine is resolved by reconciliation with evidence, never by
    resume);
  - `reconcile()`: evidence-backed settlement (`Some(true/false)`)
    transitions `Quarantined → Recovering`. Not `Ready` — a fresh
    observation and resume still gate dispatch — but the uncertainty
    that justified quarantine is resolved.

## Files changed

- `crates/vesper-bridge/src/session.rs` — quarantine-refusing resume;
  evidence-resolves-quarantine transition.
- `crates/vesper-bridge/tests/quarantine_lifecycle.rs` — new (2 tests).

## Exact evidence

| Item | Test/observation | Result |
|---|---|---|
| Quarantined resume refuses truthfully | `resume_on_a_quarantined_session_refuses_truthfully` | PASS (red on pre-repair code) |
| Evidence resolves quarantine → Recovering; resume then works | `reconciliation_with_evidence_resolves_the_quarantine` | PASS (red) |
| No existing contract regressed | `cargo test -p vesper-bridge` | **62 passed, 0 failed** (60 + 2 new) |
| Harness suites | `--features bridge --lib` / `--lib` | **142/0** / **116/0** |
| ACP / TUI | full feature-on runs | 84/0 · 395/0 |
| clippy both toolchains | bridge combo `-D warnings` | 0 errors |
| MSRV | `cargo +1.88.0 check -p vesper-bridge --tests` | clean |
| fmt / architecture / naming-guard / acceptance | repo gates | clean / 28 pkgs / 33 frozen / 23/23 |

## Honest status

- The state machine now has a complete, honest lifecycle: Ready →
  (uncertain outcome) → Quarantined → (evidence) → Recovering →
  (resume + fresh observation) → dispatchable; stop/settlement per
  increments 9–10. No reachable state lies about what the core will do.
- The production `/bridge resume` path inherits the truthful refusal
  automatically (it formats the core's `Err`), so no host change was
  needed — the fix at the core fixed every composition above it.
- Ninth real defect of the mission, same class: contracts described in
  types and docs but not implemented in behavior.

## Deviations

- None.

## Unresolved items

1. DaVinci Resolve Studio installation (Alex) — Phase 3.
2. Cua 0.28.1 install authorization (Alex) — Phase 4.
3. Native enrollment (hosting-process restart; `acceptance_enroll`
   absent from this session's tool surface).
4. Driver-level reconciliation (real evidence from a real application) —
   adapter phase.

## Readiness effect

An uncertain application outcome no longer permanently bricks a Bridge
session, and neither host can report recovery the core refuses to
deliver. The recovery model §7 requires is now actually a loop, not a
one-way trap.
