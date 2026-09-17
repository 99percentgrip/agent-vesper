# Vesper Bridge — Increment 8: emergency input release actually survives revocation (§8.4, NF-03, BR-17)

## Objective

The remaining "deferred" item (AT-37/AT-21/AT-22 adjacency) was audited
rather than deferred blindly. The audit found a real §8.4/NF-03
violation already present in the shipped fence-state code — and a unit
test that had **encoded the violated contract as correct**.

## The defect

`FenceState::revoke()` removed an input-holding lease from the live
map; `emergency_release_inputs()` scanned only live leases. Therefore a
lease revoked in an ordinary stop — whose driver never confirmed input
release — **vanished from emergency release**: the held key/button was
stranded exactly when the emergency path is supposed to still work
("Emergency input release must still work after ordinary leases are
revoked", PRD §7; NF-03; BR-17).

Worse, the in-crate unit test named
`revocation_does_not_strand_emergency_release` asserted
`released.is_empty()` after revocation — its name promised §8.4 while
its body asserted the stranding. The doc comment on
`emergency_release_inputs` claimed already-revoked holders were covered;
the code did not cover them.

## Methods and commands

- Red-first integration tests
  `crates/vesper-bridge/tests/emergency_release.rs`:
  1. revoke an input-holding lease, then emergency release must still
     report it;
  2. live and revoked input holders must both be covered;
  3. end-to-end session shape: authorize with a held-input lease,
     ordinary stop (reports the input, `Unconfirmed`), second stop must
     still surface the input.
  All three failed on the pre-repair code (`got []`, `["seat-live"]`
  missing the revoked seat, empty first-stop report).
- Repair in `FenceState`: revoked input-holding leases with unconfirmed
  release are retained in a bounded-in-practice `stranded_inputs` set;
  `emergency_release_inputs` drains them at or below the requested
  fence (once per generation; a second call does not repeat it).
- The misleading unit test was corrected to assert the §8.4 semantics
  (revoked holder reported; emergency mark advances; second call empty).

## Files changed

- `crates/vesper-bridge/src/lease.rs` — `stranded_inputs` retention,
  emergency release coverage of revoked holders, corrected unit test.
- `crates/vesper-bridge/tests/emergency_release.rs` — new (3 tests).

## Exact evidence

| Item | Test/observation | Result |
|---|---|---|
| Revoked holder reachable by emergency release | `emergency_release_survives_ordinary_revocation` | PASS (red on pre-repair code) |
| Live AND revoked covered together | `emergency_release_covers_both_live_and_revoked_input_holders` | PASS (red) |
| Session stop path keeps the input visible | `emergency_release_still_runs_after_ordinary_stop_revoked_the_lease` | PASS (red) |
| Corrected unit contract incl. no double release | `lease::tests::revocation_does_not_strand_emergency_release` | PASS |
| No existing contract regressed | `cargo test -p vesper-bridge` | **58 passed, 0 failed** (55 + 3 new) |
| Harness unaffected | `cargo test -p vesper-harness --features bridge --lib` | 137 passed, 0 failed |
| clippy both toolchains | bridge+harness combo `-D warnings` | 0 errors (1.88.0 and stable) |
| MSRV | `cargo +1.88.0 check -p vesper-bridge --tests` | clean |
| fmt / architecture / naming-guard / acceptance | repo gates | clean / 28 pkgs / 33 frozen / 23/23 |

## Honest status

- This closes the **deterministic core** of §8.4/NF-03. What remains
  NOT TESTED for AT-21/AT-22 is the driver/host half: a real driver
  acknowledging release, the watchdog, and host keybinding surfaces —
  adapter/host territory behind the blocked lanes.
- The lesson recorded: a test whose name promises a safety property must
  be re-read against the property, not trusted because it passes. Three
  prior reports (phase1/2/2c) cited "leases/emergency paths covered"
  while this defect sat beneath a green test named for the property.

## Deviations

- None. `stranded_inputs` retention is bounded by construction: entries
  drain on emergency release and never accumulate for confirmed fences;
  the set only ever holds revoked-unconfirmed holders.

## Unresolved items

1. DaVinci Resolve Studio installation (Alex) — Phase 3.
2. Cua 0.28.1 install authorization (Alex) — Phase 4.
3. Native enrollment (hosting-process restart; no `acceptance_enroll`
   on this session's tool surface).
4. Driver/host half of AT-21/22 (acknowledged release, watchdog) —
   adapter phase.

## Readiness effect

The stop path can no longer strand driver-held inputs in any composition
that goes through `FenceState` — the specific unsafe state §8.4 exists
to prevent — and the test that hid it now enforces the property it
names.
