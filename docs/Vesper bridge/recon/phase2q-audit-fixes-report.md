# Vesper Bridge — Increment 16: full-mission-audit fixes (all findings)

## Objective

Fix every finding of the Phase 0→2p full-mission audit
(`full-mission-audit.md`): 3 CRITICAL, 7 HIGH, 10 MEDIUM, 7 LOW —
red-first, preserving every existing contract, with no adapter yet in
existence so all fixes land on the deterministic core and composition.

## Methods

Red-first: `crates/vesper-bridge/tests/audit_fixes.rs` written BEFORE any
fix; confirmed failing (8 missing APIs, 6 findings) on the pre-fix code;
10/10 green after. Legacy suites updated only where they encoded the
broken behavior (hard-coded authority epoch `1`, fence-0 re-dispatch,
duplicate-arm shapes).

## Fixed (core: `vesper-bridge`)

| Fix | Mechanism |
|---|---|
| **C1** lease leak on post-admit denial | steps 7/8 revoke via `FenceState::revoke_by_token` on DuplicateSuppressed, ReconcileBeforeRetry and journal-failure paths |
| **C2+M10** revalidate-before-dispatch (PRD §6) | `BridgeSession::revalidate(lease, generation, elapsed_ms)`: lease liveness (`validates()` now HAS a consumer), generation, timeout policy, lease expiry |
| **C3** quarantine erasure on close | `close()` returns `CloseReport { was_quarantined, inputs_pending_release, outstanding_jobs }`; never silently erases |
| **H1** cross-session emergency sweep | `LeaseState.owner` (session id) stamped at admission; `emergency_release_inputs_owned(owner)`; the `u64::MAX` global sweep removed from all production paths |
| **H5** same-observation mutation replay | `latest_mutation_observation_revision`: a mutating dispatch needs an observation strictly newer than its predecessor mutation's; read-only reuse stays allowed |
| **H2** authority epoch | minted on `connect()` and `resume()` (`checked_add`, fail-closed at u32::MAX); stale epochs refuse at step 5b |
| **H3** error truth | `BridgeError::InvalidState` for connect-on-attached (was `TransportUnavailable`) |
| **M7** partial evidence | `IntentRecord.partial_evidence` (4 KiB char-boundary bounded), journaled by `settle_partial(journal, …)` |
| **M9** confirmation latch | admitting an input-holding lease clears `input_release_confirmed` |
| **M6** silent job eviction | `evicted_jobs` counter surfaced in the stop report ("list is not exhaustive") |
| **L2** write-only mark | `emergency_released_through()` surfaced in the stop report |
| **L4** header drift | session module header now cites BR-21/NF-02/NF-05 |

## Fixed (composition: `vesper-harness`, both hosts)

| Fix | Mechanism |
|---|---|
| **C2 wiring** | `bridge_execute` runs `revalidate()` after authorize, before the (future) adapter handoff |
| **C3 surfacing** | `bridge_disconnect` + `/bridge disconnect` report quarantine/inputs/jobs from the CloseReport |
| **H4** `/bridge disconnect` executes | both hosts call the live service (parity with stop/resume; BR-21) |
| **H4/L3** no panic paths | `unreachable!()` replaced by truthful "host did not wire that path" text; `every_documented_verb_answers_without_panicking` test |
| **H6** observe identity | observe returns `{id, revision, captured_at_ms, kind, degraded_capture}` — the model can now bind plans (§6) |
| **H7** lease denial detail | `LeaseConflict{holder, expires_in_ms}` and `StaleFence{observed, required}` reach the model text |
| **M1** argument bound | 64 KiB at `bridge_execute` (`MAX_ARGUMENT_BYTES`) |
| **M2** discarded arguments | caller's arguments flow into the validated `OperationSpec` |
| **M4** budget gate | observe refuses over-budget (NF-07) and degraded (AT-14) observations at admission |
| **M8/M8b** surface truth | help regenerated from real verbs, case-insensitive; TUI/ACP patterns unified |
| **L1** string drift | `BRIDGE_NO_ADAPTER_DISCOVERY` single constant |

## Documented, not coded (honest boundaries)

- **M3** journal double-read: single-owner session + tokio Mutex is the
  enforced precondition; recorded in authorize's contract.
- **M5** generation advance: adapter-phase prerequisite (discovery→attach
  mints `Generation::next()` on new process-start evidence).
- **C3 producer half**: production quarantine-on-timeout is adapter-phase
  wiring (no adapter exists to time out); the reporting half is fixed now.
- **L5** unused derives: retained deliberately for Phase 3 durable sessions.

## Defects introduced and caught during the fix itself

1. Nested fence-lock deadlock in the M9 latch check (locked `fences` while
   the step-6 guard held it) — fixed by evaluating the latch condition
   BEFORE taking the lock; caught by the audit tests hanging.
2. Duplicated `Granted` match arm from a partial edit — caught by the
   compiler's unreachable-pattern warning; removed.

Both are recorded as evidence that the red-first tests did their job.

## Verification receipts (fresh, this increment)

```
cargo test -p vesper-bridge                      → 78 passed, 0 failed (68 + 10 audit)
cargo test -p vesper-harness --features bridge --lib → 144 passed, 0 failed (+2 command)
cargo test -p vesper-harness --lib               → 116 passed, 0 failed (baseline unchanged)
cargo test -p agent-vesper-acp --features bridge → 84 passed, 0 failed
cargo test -p agent-vesper-tui   --features bridge → 395 passed, 0 failed
cargo test -p vesper-bridge -p vesper-harness --features bridge → 256 passed, 0 failed
cargo clippy (bridge/harness/acp/tui, --features bridge, --all-targets, -D warnings) → 0 errors
cargo fmt                                        → clean
cargo xtask architecture                         → 28 packages validated
cargo xtask naming-guard                         → clean (33 frozen)
cargo xtask acceptance                           → 23/23 exact cases
```

## Files changed

- `crates/vesper-bridge/src/session.rs` — C1/C2/C3/H1/H2/H3/H5/M6/M9/L4
- `crates/vesper-bridge/src/lease.rs` — owner field, revoke_by_token,
  owner-scoped release/settle, stranded-survival, L2
- `crates/vesper-bridge/src/journal.rs` — M7 partial evidence + reseed_for_tests
- `crates/vesper-bridge/src/error.rs` — H3 InvalidState
- `crates/vesper-bridge/tests/audit_fixes.rs` — NEW, 10 red-first tests
- `crates/vesper-bridge/tests/*` — owner/epoch fields in fixtures (no
  assertion weakened; only broken-contract literals updated)
- `crates/vesper-harness/src/bridge_service.rs` — C2 wiring, C3 surfacing,
  H4/H6/H7/M1/M2/M4/M6/L1, epoch+fence in dispatch, argument bound
- `crates/vesper-harness/src/bridge_command.rs` — H4/L3/M8 rewrite
- `crates/vesper-harness/src/lib.rs` — `bridge_disconnect()`
- `apps/agent-vesper-acp/src/lib.rs` — `/bridge disconnect` executes
- `apps/agent-vesper-tui/src/main.rs` — same (BR-21 parity)
- `docs/Vesper bridge/recon/full-mission-audit.md` — fix-status table
- `docs/Vesper bridge/PRD-ENROLLMENT-NOTE.md` — L6 resolution banner

## Deviations

None. Every finding the user accepted ("please fix all gaps") is fixed or
explicitly documented as adapter-phase debt with its trigger named.

## Readiness effect

The deterministic Bridge contract layer now enforces the PRD's §6
revalidation sentence, cannot leak authority on a denied dispatch, cannot
sweep another session's inputs, cannot replay a mutation against a stale
observation, cannot reuse a pre-stop approval, and reports every
unresolved state honestly. Phase 3 (Resolve) attaches to a hardened gate.
