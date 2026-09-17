# Vesper Bridge — Increment 5: AT-01 OS-observation lane (ACP host)

## Objective

Close the last unblocked item from phase2b/2d: AT-01's OS-level
host-startup observation — proving, against kernel accounting, that a
Bridge-enabled ACP host which never invokes a Bridge tool holds **no
child processes and no sockets beyond the disabled baseline**, and no
durable Bridge state.

## Methods and commands

- New integration test
  `apps/agent-vesper-acp/tests/bridge_at01_os_observation.rs`, gated
  `#![cfg(all(feature = "bridge", unix))]` (feature-off builds compile
  zero bridge tests; the compiled-out proof stays in phase2).
- Two real `agent-vesper-acp` processes via the existing
  `ProcessHarness` (isolated HOME/XDG/env, signed-out OpenAI fixture,
  non-blocking model fixture listener):
  1. **enabled**: workspace `.agent-vesper/bridge-settings.json`
     `{"enabled": true}`; `/bridge status` answered "enabled";
  2. **disabled**: identical spawn, no settings file; `/bridge status`
     answered "disabled".
- OS observation after the command round-trips:
  - child processes: `/proc/<pid>/task/*/children` (kernel accounting,
    no ptrace — `ptrace_scope` irrelevant);
  - TCP sockets: `ss -tnp -H` filtered by the host pid;
  - durable state: absence of `.agent-vesper/bridge-state` in the
    isolated root.

## Files changed

- `apps/agent-vesper-acp/tests/bridge_at01_os_observation.rs` (new).

## Exact evidence

| Item | Test/observation | Result |
|---|---|---|
| Enabled host holds no child processes | `/proc/<pid>/task/*/children` after `/bridge status` | PASS (empty set) |
| Disabled baseline holds no child processes | same | PASS (empty set) |
| Enabled host adds no TCP socket vs disabled baseline | `ss -tnp -H` pid-filtered, set-equality vs baseline | PASS (both empty at observation point — the model listener is non-blocking and unused) |
| No durable Bridge state created | `.agent-vesper/bridge-state` absent | PASS |
| Settings truthfully report both states | `/bridge status` transcript text | PASS ("enabled" / "disabled") |
| Feature-off gating | `cargo test -p agent-vesper-acp --test bridge_at01_os_observation` (no feature) | PASS — 0 tests compiled in |
| Full ACP suite with bridge | `cargo test -p agent-vesper-acp --features bridge` | 84 passed, 0 failed |
| ACP feature-off suite | `cargo test -p agent-vesper-acp` | 83 passed, 0 failed |
| clippy both toolchains | `cargo clippy -p agent-vesper-acp --features bridge --all-targets -- -D warnings` (1.88.0 and stable) | 0 errors |
| fmt | `cargo fmt --all -- --check` | clean |
| architecture | `cargo xtask architecture` | 28 packages validated |
| naming-guard | `cargo xtask naming-guard` | clean (33 frozen) |
| acceptance gate | `cargo xtask acceptance` | 23/23 exact cases passed |

## Scope honesty (what AT-01 now covers, and what it does not)

- **Now covered (three evidence classes):** registry/advertisement
  (phase2), compiled-out builds (phase2/2b), and this increment's
  OS-process/socket/durable-state observation of a real ACP host with
  Bridge **enabled but idle**.
- **Still NOT TESTED:** the TUI host's OS observation (this lane used
  the ACP process, which is the automatable one); capture, compositor,
  input and adapter lanes — none exist in this build and none is
  simulated as production behavior. The AT-01 row's status moves from
  "NOT TESTED (OS observation)" to **partial — process/socket/state
  proven for the ACP host**, with the TUI observation and adapter lanes
  remaining open.
- No acceptance scenario is claimed beyond AT-01's observation half.

## Deviations

- The first draft asserted on the `session/prompt` response object; the
  ACP contract delivers command output via `session/update`
  notifications, so the assertion reads the transcript via
  `support::update_texts` (same pattern as `acceptance_controls.rs`).
  No production code changed.

## Unresolved items

1. DaVinci Resolve Studio installation (Alex) — Phase 3 unblocks.
2. Cua 0.28.1 install authorization (Alex) — Phase 4 unblocks.
3. Native enrollment freeze + verification (hosting-process restart on
   the repaired binary; this session's tool surface does not expose
   `acceptance_enroll`).
4. TUI-host OS observation for AT-01 parity (lower priority; the
   composition and registry paths are already proven shared).

## Readiness effect

AT-01's OS-observation gap for the automatable host is closed with a
repeatable, kernel-accounted test that fails the moment an idle
Bridge-enabled host acquires a process, socket, or durable state. The
remaining unexecuted work all sits behind the two blocked lanes.
