# Phase 3 — §7 Dispatch-Integrity Hardening (Session 2w)

**Date:** 2026-09-16 · **Status: PASS — timeout honesty and semantic duplicate suppression enforced, live re-verified**

## Objective

Close the two §7 invariants flagged as gaps in 2v: a timeout after a
possibly-committed mutation must classify `unknown_outcome` (never
`failed`), and a retried unsettled non-idempotent operation must be refused
with reconciliation guidance instead of silently re-running.

## Two real holes found — and one was deep

### Hole 1: timeout settled as `Failed` (shallow, service-level)

Every adapter `Err` — including `Timeout` — settled `Failed` and reported
"adapter dispatch failed". A blind retry after that could double-apply a
mutation the application actually performed.

**Fix:** timeout/transport-class adapter errors settle `UnknownOutcome` and
the tool text says: *"the operation may or may not have been applied —
reconcile the application state before retrying (unknown_outcome)"*.
Pre-dispatch errors (invalid arguments, missing dependency) stay honest
`Failed`.

### Hole 2: duplicate suppression was keyed on the wrong identity (deep, core-level)

`classify_retry` looked up records by `OperationRequestId` — but every
dispatch mints a **fresh** sequence id. A real-world retry (same op, same
args, new attempt id) was **always `Fresh`**. The BR-16 protection could
only fire on an exact attempt-id replay, which never happens outside a
contrived test. The journal had the *right data* under the *wrong key* for
six increments.

**Fix:** `classify_semantic_retry` + `operation_identity` (capability +
canonical arguments). Step **5a** in `authorize()`, deliberately placed
**before** the freshness checks: a retried unsettled mutation must be told
to *reconcile* (actionable), not merely to *observe again* — a freshness
denial would invite exactly the retry loop §6.6 exists to prevent.
`JournalPort` gained `recent_intents()` (bounded window, newest first);
`MemoryJournal` implements it within its existing retention cap.

## Red-first receipts

- `timeout_after_mutation_settles_unknown_outcome_not_failed` — red (my
  first "green" was accidental: the fake's detail string contained the
  assertion text — caught it, tightened the test to the real marker, then
  it was genuinely red), then green after the service fix.
- `duplicate_of_unsettled_mutation_is_refused_with_reconcile_guidance` —
  red (got generic freshness text), green after 5a ordering.
- Three existing tests legitimately re-shaped: C1's id-collision path and
  H5's freshness pin now use *different* capabilities so the contract each
  tests is reachable — same-op replays hit 5a (their own new contract).
  `manifest()` gained `media.marker.add` for this.

## Verification (all fresh)

| Gate | Result |
|---|---|
| `vesper-bridge` (5 suites) | **48/0** (29+10+4+3+2) |
| harness `--features bridge --lib` | **149/0** (+2 hardening) |
| harness feature-off | **122/0** |
| clippy stable **and** 1.88.0, both crates, all targets | 0 errors |
| fmt / architecture / acceptance | clean / 28 pkgs / **23/23** |
| **LIVE** `live_mpris_player_control` | **PASS** |
| **LIVE** `live_resolve_worker_status_roundtrip` | **PASS** |

## Honest limitations

- Semantic suppression compares canonical argument JSON; semantically
  equivalent-but-textually-different args (e.g. reordered keys) are
  handled (serde_json sorts), but *intention* equivalence ("render again")
  is not inferred — by design, not oversight.
- `recent_intents()` scans the bounded window linearly — fine at the
  512-record cap, not a general-purpose index.
- Reconciliation itself (adapter probing app state to resolve
  `UnknownOutcome` into `Applied`/`Failed`) remains the next hardening
  step: the guidance is now correct, the automated probe is future work.

## Files

- `crates/vesper-bridge/src/journal.rs` — `classify_semantic_retry`,
  `operation_identity`, `recent_intents()` port method
- `crates/vesper-bridge/src/session.rs` — step 5a; import ordering
- `crates/vesper-harness/src/bridge_service.rs` — timeout→`UnknownOutcome`
  settlement + reconciliation tool text
- `crates/vesper-harness/src/bridge_hardening_tests.rs` — new (2 tests)
- `crates/vesper-bridge/tests/audit_fixes.rs` — C1/H5 re-shaped,
  `media.marker.add` capability added

## Verdict

**PASS.** The two ways Bridge could lie about a mutation — calling an
uncertain timeout a failure, and re-running a possibly-committed operation
— are now structurally impossible, with the deep one (wrong duplicate key)
found only because the red test refused to accept my first accidental
green.
