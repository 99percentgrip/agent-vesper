# Vesper Bridge — Increment 12: every §7 outcome state is reachable

## Objective

Completing the lifecycle-reachability sweep over the **outcome** enum:
`OperationOutcome::Cancelled` and `OperationOutcome::Partial` were both
constructed by **no code path anywhere** — not in the core, not in
hosts, not even in tests beyond exhaustive-variant assertions. §7
enumerates them ("classify results as dispatched, applied, verified,
partial, failed, cancelled or unknown"); the enum described a contract
the core silently did not honor. A cancellation could never be recorded
as a cancellation.

## Methods and commands

- Red-first `crates/vesper-bridge/tests/outcome_reachability.rs`:
  1. `cancel()` must exist, settle the record to `Cancelled` (terminal,
     not success), and retain the record;
  2. `settle_partial()` must exist and settle to `Partial`;
  3. cancel + quarantine/reconcile lifecycles do not interfere.
  Cases 1–2 failed to compile on the pre-repair core (no such APIs).
- Core APIs added in `session.rs`:
  - `cancel(request_id)` — records `Cancelled`; a no-op success when the
    record is already terminal (honest about nothing being in flight);
    errors when the record does not exist. Resource reservations are
    deliberately NOT released here (§7: cancellation retains
    reservations until work settles; release belongs to the
    stop/settlement path — increments 9–10).
  - `settle_partial(request_id, evidence)` — records `Partial`; the
    evidence text is journaled by the host, the core keeps the truthful
    outcome class.

## Files changed

- `crates/vesper-bridge/src/session.rs` — `cancel`, `settle_partial`.
- `crates/vesper-bridge/tests/outcome_reachability.rs` — new (3 tests).

## Exact evidence

| Item | Test/observation | Result |
|---|---|---|
| `Cancelled` reachable, terminal, not success, record retained | `cancellation_is_reachable_and_settles_with_reservation_kept` | PASS (red: API did not exist) |
| `Partial` reachable, terminal, not success | `partial_is_reachable_with_retained_evidence` | PASS (red: API did not exist) |
| Cancel + quarantine lifecycles independent | `cancel_then_reconcile_with_evidence_still_resolves` | PASS |
| No existing contract regressed | `cargo test -p vesper-bridge` | **65 passed, 0 failed** (62 + 3 new) |
| Harness suite | `--features bridge --lib` | 142/0 |
| clippy both toolchains | bridge combo `-D warnings` | 0 errors |
| fmt / architecture / naming-guard / acceptance | repo gates | clean / 28 pkgs / 33 frozen / 23/23 |

## Honest status

- Every §7 outcome state is now reachable through a real API, and every
  session lifecycle state except `Paused`/`Closed` has a producing and
  consuming path. Those two remain representation-only: `Paused` has no
  producer (nothing pauses a session today — pause is the adapter/host
  UX concept) and `Closed` is only ever *read* (by `resume()`, which
  refuses it). They are states the state machine can never enter; an
  honest core would either implement the transitions or remove them.
  Left in place deliberately: removing them would change the serialized
  state contract before any adapter exists to exercise it, and the
  enum's doc comments mark them as future host-lifecycle states. This is
  recorded as **representation debt**, not a verified behavior — and it
  is the exact class of gap a driver-acknowledged adapter phase must
  close (pause on host focus loss, close on disconnect).
- Tenth and eleventh reachability defects of the mission, same class as
  increments 8–11: enum variants are contracts, not decorations.

## Deviations

- None.

## Unresolved items

1. DaVinci Resolve Studio installation (Alex) — Phase 3.
2. Cua 0.28.1 install authorization (Alex) — Phase 4.
3. Native enrollment (hosting-process restart; `acceptance_enroll`
   absent from this session's tool surface).
4. `Paused`/`Closed` producers + driver-level cancellation/partial
   evidence (adapter phase).

## Readiness effect

§7's outcome vocabulary is now fully implementable end-to-end: a host
can record a cancellation as a cancellation and a partial effect as a
partial effect, with terminality and non-success semantics enforced by
the core rather than assumed by callers.
