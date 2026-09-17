# Vesper Bridge — Phase 1 evidence completion: deterministic fake driver + MSRV

## Objective

Close the two Phase 1 gaps left open in the earlier reports: (a) the
deterministic **fake driver** proving protocol/state behavior end-to-end
through the pure core (PRD Phase 1: "fake driver … deterministic tests
for denial, stale identity, duplicate requests, unknown outcomes and
shutdown"), and (b) the MSRV 1.88 gate for every new unit.

## Methods and commands

- `crates/vesper-testkit/src/bridge_fake.rs` — `FakeApplication`: a
  visibly fake, test-only driver. Restart bumps a **generation**
  (modeling PID/window reuse), applied writes advance a revision, and its
  manifest keeps `unknown` deliberately distinct from `native`. It lives
  in the non-production testkit per its contract; production crates
  cannot depend on it.
- `crates/vesper-testkit/tests/bridge_fake_scenarios.rs` — six
  end-to-end scenarios through `BridgeSession`:
  happy-path verify-once; restart-invalidated binding (AT-03 shape);
  ack-without-effect fails verification (AT-15 shape);
  disconnect-after-create reconcile-before-retry (AT-19 shape);
  cross-session two-writers lease conflict via a shared fence
  coordinator (AT-16 shape); unknown-operation pre-dispatch rejection
  (AT-05 shape).
- Core hardening driven red-first by those scenarios:
  - `BridgeSession::verify` now **requires independent evidence**
    (`postcondition_holds: bool`); a driver `Applied` alone can no longer
    promote to `Verified` (BR-12/NF-13).
  - Lease exclusivity is **per resource, not per session**:
    `with_shared_fences` lets the hosted layer coordinate sessions over
    one `FenceState` (BR-14/AT-16).
- MSRV: `cargo +1.88.0 check` for `vesper-bridge`, `vesper-testkit`,
  `vesper-harness` (both feature states) — all clean.

Gates:
- `cargo test -p vesper-bridge` → 46 passed (29 lib + 17 gate).
- `cargo test -p vesper-testkit` → 29 lib + 6 fake scenarios passed.
- `cargo test -p vesper-harness --features bridge --lib` → 131 passed.
- `cargo clippy -p vesper-bridge -p vesper-testkit -p vesper-harness
  --all-targets --features vesper-harness/bridge -- -D warnings` → clean.
- `cargo fmt` → clean; `cargo xtask architecture` → 28 packages;
  `cargo xtask naming-guard` → clean.

## Files changed

- `crates/vesper-testkit/src/bridge_fake.rs`, `tests/
  bridge_fake_scenarios.rs`, `Cargo.toml`, `src/lib.rs` (fake export).
- `crates/vesper-bridge/src/session.rs` (verify-evidence contract,
  shared-fence coordinator), `tests/session_gate.rs` (two-arg verify).
- `xtask/src/main.rs` (testkit allowlist entry for vesper-bridge).
- `crates/vesper-testkit/AGENTS.md` — noted the fake-driver contract.
- Docs: this report + evidence index.

## Exact evidence

| Scenario | Test | Result |
|---|---|---|
| Happy path: ack→Applied, evidence→Verified, exactly one write | `fake_driver_happy_path_verifies_exactly_once` | PASS |
| AT-03 shape: restart invalidates binding | `fake_restart_invalidates_the_old_binding` | PASS |
| AT-15 shape: success string without effect never verifies | `fake_success_string_without_effect_fails_verification` | PASS |
| AT-19 shape: unknown outcome ⇒ reconcile before retry | `fake_disconnect_after_create_reconciles_before_retry` | PASS |
| AT-16 shape: two writers, one document, shared coordinator | `fake_two_writers_cannot_hold_one_document` | PASS |
| AT-05 shape: unknown op rejected before dispatch | `fake_unknown_operation_is_rejected_before_any_dispatch` | PASS |
| MSRV 1.88 | `cargo +1.88.0 check` × 4 targets | PASS |

## Honest status

- These are **deterministic contract/protocol tests with a fake driver**.
  They certify Bridge state behavior only; no real application,
  transport or capture exists yet (Phases 3–4), and none of this may be
  cited as application-control capability.
- The 44 AT rows keep their per-lane statuses: these fake-driver tests
  provide the "deterministic reference driver" evidence the Phase 1 gate
  names; live/native lanes remain **NOT TESTED/BLOCKED**.
- Native enrollment remains unfrozen (host binary predates the ceiling
  repair; retries exhaust the enrollment window during contract
  preparation). No reduced scope was enrolled; no completion is claimed.

## Deviations

- None this increment; the earlier `verify()` signature change is a
  strictness increase (driver acks alone no longer verify) applied
  red-first across all call sites.

## Unresolved items

1. Native enrollment freeze + verification (host restart on the repaired
   binary).
2. Host-startup OS observation for AT-01 (process/network traces).
3. Phase 3 BLOCKED (no Resolve); Phase 4 BLOCKED (no Cua authorization).

## Readiness effect

Phase 1's named deliverable set is now complete with deterministic
evidence and the MSRV gate satisfied for every new unit. The next
permitted step is Phase 3's Resolve lane (on installation) or Phase 4's
driver lane (on authorization).
