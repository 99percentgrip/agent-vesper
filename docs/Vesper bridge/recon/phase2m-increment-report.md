# Vesper Bridge — Increment 13: session lifecycle complete; disconnect surfaces what it drops

## Objective

Close the last reachability debt recorded in increment 12: `Paused` and
`Closed` had **no producers** — the session state machine could never
enter either state. And the audit of the producing side found a second
defect: production `bridge_disconnect` did `guard.take()` and silently
discarded any held-input leases and outstanding jobs, reporting a flat
"session closed" regardless of what was still unresolved.

## Methods and commands

- Red-first `crates/vesper-bridge/tests/session_lifecycle_completion.rs`:
  1. `pause()` from Ready/Recovering → `Paused`; resume returns to a
     dispatchable state;
  2. pause from Quarantine refuses — pause is not a resolution path
     (BR-18, increment 11);
  3. `close()` → `Closed`, terminal, idempotent; resume refuses.
  Cases 1–3 failed to compile on the pre-repair core (`pause`/`close`
  did not exist).
- Core: `pause()` (only from Ready/Recovering; anything stronger
  semantics-wise refuses with `TargetChanged`) and `close()` (sets
  `Closed` + closes admission; idempotent).
- Production disconnect now inspects before dropping: surfaces inputs
  still pending emergency release and outstanding jobs in the closing
  message (`fences_for_release()` exposure), then `close()`s the
  session. The clean case still reports the clean message — the warning
  appears only when something genuinely remains.

## Files changed

- `crates/vesper-bridge/src/session.rs` — `pause`, `close`,
  `fences_for_release`.
- `crates/vesper-bridge/tests/session_lifecycle_completion.rs` — new.
- `crates/vesper-harness/src/bridge_service.rs` — disconnect surfaces
  unresolved inputs/jobs before closing.

## Exact evidence

| Item | Test/observation | Result |
|---|---|---|
| `Paused` reachable; resume returns dispatchable | `pause_is_reachable_refuses_dispatch_and_resumes` | PASS (red: API absent) |
| Pause cannot weaken quarantine | `pause_from_quarantine_refuses_honestly` | PASS |
| `Closed` reachable, terminal, idempotent; resume refuses | `close_is_reachable_and_terminal` | PASS (red: API absent) |
| Disconnect surfaces unresolved state | code path + wording (warning only when inputs/jobs remain) | PASS (no flat "closed" lie) |
| No existing contract regressed | `cargo test -p vesper-bridge` | **68 passed, 0 failed** (65 + 3 new) |
| Harness suites | `--features bridge --lib` / `--lib` | **142/0** / **116/0** |
| ACP / TUI | full feature-on runs | 84/0 · 395/0 |
| clippy both toolchains | bridge combo `-D warnings` | 0 errors |
| MSRV | `cargo +1.88.0 check -p vesper-bridge --tests` | clean |
| fmt / architecture / naming-guard / acceptance | repo gates | clean / 28 pkgs / 33 frozen / 23/23 |

## Honest status

- The session state machine is now fully reachable: every state in
  `ApplicationSessionState` and every outcome in `OperationOutcome` has
  both a producer and a consumer. The reachability sweep that began at
  increment 8 (unreachable `Released`) is complete — twelve defects of
  this one class, all found by auditing types against behavior, all
  fixed red-first.
- What this does NOT claim: pause-on-focus-loss and close-on-disconnect
  *host wiring* (when a host actually calls these in response to OS
  events) remains adapter/host UX work behind the blocked lanes. The
  core contract is complete; the host triggers are future composition.
- `Paused`'s serialization meaning is unchanged (variant existed; only
  reachability was added), so no migration concern.

## Deviations

- None.

## Unresolved items

1. DaVinci Resolve Studio installation (Alex) — Phase 3.
2. Cua 0.28.1 install authorization (Alex) — Phase 4.
3. Native enrollment (hosting-process restart; `acceptance_enroll`
   absent from this session's tool surface).
4. Host trigger wiring for pause/close on real OS events;
   driver-acknowledged evidence, watchdog, AT-37 host observation
   queues (adapter phase).

## Readiness effect

The deterministic Bridge contract layer is now internally complete: no
lifecycle state, outcome class, settlement terminal, or budget exists
only in types. A disconnect can no longer make held inputs or running
jobs vanish with the session object — they are surfaced in the closing
report, exactly as §8.4 and BR-13 require.
