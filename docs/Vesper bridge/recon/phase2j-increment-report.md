# Vesper Bridge — Increment 10: input-release settlement exists (BR-17)

## Objective

Continuing the sweep for contract states that exist in types but are
unreachable in code: `InputLeaseState::Released` was constructed by **no
code path anywhere**. `stop()` always set `Unconfirmed`, the doc
comment said "host fills after driver ack", and the fill mechanism had
never been written. BR-17 settlement reporting could never terminate —
every stop would re-warn an unconfirmed input state forever, and the
emergency-release path (increment 8) would re-report settled inputs on
every subsequent stop.

## Methods and commands

- Red-first `crates/vesper-bridge/tests/input_release_settlement.rs`:
  1. stop reports `Unconfirmed` with inputs pending; after the host
     confirms, a later stop reports `Released` and no longer re-reports
     the settled inputs for emergency release;
  2. confirming without a prior stop is harmless and truthful.
  Both failed on the pre-repair session (`confirm_input_release` did not
  exist; the second assertion then failed again because the fence state
  kept re-reporting).
- Core: `BridgeSession::confirm_input_release()` — an **explicit host
  act** after driver acknowledgement (§7: an ack string is not a
  postcondition; the host owns the transition) — marks settlement and
  clears both live input-holding leases and stranded-input records via
  the new `FenceState::settle_input_release()`.
- Production surface: `BridgeToolService::confirm_input_release()` →
  `HarnessToolService::bridge_confirm_input_release()` → both hosts'
  `/bridge release confirmed` verb; stop reports now distinguish
  "inputs released" from "input release UNCONFIRMED — unsafe input
  state possible"; the shared command help lists the verb.

## Files changed

- `crates/vesper-bridge/src/session.rs` — `input_release_confirmed`
  state, `confirm_input_release()`, truthful stop reporting.
- `crates/vesper-bridge/src/lease.rs` — `FenceState::settle_input_release()`.
- `crates/vesper-bridge/tests/input_release_settlement.rs` — new.
- `crates/vesper-harness/src/bridge_service.rs` — production
  confirmation + settled/unconfirmed stop wording.
- `crates/vesper-harness/src/lib.rs` — `bridge_confirm_input_release`.
- `apps/agent-vesper-tui/src/main.rs`, `apps/agent-vesper-acp/src/lib.rs`
  — `/bridge release confirmed` verb.
- `crates/vesper-harness/src/bridge_command.rs` — help text.

## Exact evidence

| Item | Test/observation | Result |
|---|---|---|
| `Released` reachable; settled inputs not re-reported | `stop_reports_unconfirmed_then_confirmation_reaches_released` | PASS (red on pre-repair code) |
| Confirmation without prior stop harmless | `confirmation_without_a_prior_stop_is_harmless_and_truthful` | PASS |
| No existing contract regressed | `cargo test -p vesper-bridge` | **60 passed, 0 failed** (58 + 2 new) |
| Harness suites | `--features bridge --lib` / `--lib` | **142/0** / **116/0** |
| ACP / TUI / bridge | full feature-on runs | 84/0 · 395/0 · 60/0 |
| Both hosts build | `cargo build` both apps, both feature states | 0 errors |
| clippy both toolchains | bridge combo `-D warnings` | 0 errors |
| fmt / architecture / naming-guard / acceptance | repo gates | clean / 28 pkgs / 33 frozen / 23/23 |
| Real TUI end-to-end | PTY `/bridge release confirmed` | PASS — "Bridge: no session is connected; nothing to confirm." (truthful for the no-adapter build) |

## Honest status

- BR-17's settlement terminal state now exists, is reachable from both
  hosts, and clears the records it settles. The **driver-acknowledged**
  half (a real driver confirming release) remains adapter-phase
  territory — this build has no driver, and no fabricated confirmation
  exists: the host verb is an explicit act, not an automatic ack.
- Eighth real defect of this mission class found by auditing enum
  variants for reachability: types that describe contracts are not
  evidence the contracts are implemented.

## Deviations

- None.

## Unresolved items

1. DaVinci Resolve Studio installation (Alex) — Phase 3.
2. Cua 0.28.1 install authorization (Alex) — Phase 4.
3. Native enrollment (hosting-process restart; `acceptance_enroll`
   absent from this session's tool surface).
4. Driver-acknowledged release + watchdog (adapter phase).

## Readiness effect

The stop/settlement state machine is now complete and honest in both
hosts: Unconfirmed on stop, explicit host confirmation to Released,
settled inputs never re-warned. No contract state remains unreachable.
