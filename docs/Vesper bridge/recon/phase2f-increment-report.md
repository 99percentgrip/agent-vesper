# Vesper Bridge — Increment 6: TUI AT-01 lane + a real host-parity gap repaired

## Objective

Close the final open item from phase2e — the TUI-host OS observation for
AT-01 — and, in doing so, repair the defect the new test immediately
exposed: the TUI host **silently dropped `/bridge` commands** because
`pending_bridge_command` was set by dispatch but never consumed by the
main loop.

## Methods and commands

- New PTY observation script
  `apps/agent-vesper-tui/tests/bridge_at01_pty.py` (stdlib only, mirrors
  the house `settings_pty.py` harness): spawns the **real TUI binary**
  built with `--features bridge` in an isolated HOME/XDG/proxy-poisoned
  environment, enters the conversation, runs `/bridge status`, then
  observes the live process via kernel accounting:
  - children: `/proc/<pid>/task/*/children`;
  - sockets: `ss -tnp -H` pid-filtered;
  - runs both passes (settings `enabled: true` vs no settings file) and
    asserts identical zero footprint plus truthful status text.
- `cargo build -p agent-vesper-tui --features bridge`, then
  `python3 apps/agent-vesper-tui/tests/bridge_at01_pty.py target/debug/agent-vesper-tui`.

## The defect the test found (and why prior receipts missed it)

`dispatch.rs` stored `CommandOutcome::Bridge` into
`state.pending_bridge_command` (phase2b), but no consumer existed in
`main.rs` — the value was written and then dropped at the idle-turn
boundary. The ACP host had its own working `bridge_host::command`, so
process-transcript advertisement tests passed and TUI command-surface
registry tests passed, while the TUI **user-visible behavior** was a
silent no-op. Evidence class lesson recorded: advertisement and
compilation prove the surface exists, not that it answers.

## Repair

- `crates/vesper-harness/src/bridge_command.rs` (new, `bridge`-gated):
  the single `/bridge` answer implementation, moved from the ACP's
  private `bridge_host.rs`, with 3 unit tests (disabled-without-root,
  unknown-argument surface listing, no answer claiming an application
  action).
- `apps/agent-vesper-acp/src/bridge_host.rs` now re-exports the shared
  implementation (host parity BR-21: one text source, no drift).
- `apps/agent-vesper-tui/src/main.rs`: consumes
  `pending_bridge_command` at the idle-turn boundary and surfaces the
  shared answer as status text (feature-gated, placed beside the swarm
  consumer).

## Files changed

- `apps/agent-vesper-tui/tests/bridge_at01_pty.py` (new)
- `crates/vesper-harness/src/bridge_command.rs` (new)
- `crates/vesper-harness/src/lib.rs` (module registration)
- `apps/agent-vesper-acp/src/bridge_host.rs` (shared re-export)
- `apps/agent-vesper-acp/src/lib.rs` (root argument for status)
- `apps/agent-vesper-tui/src/main.rs` (missing consumer added)

## Exact evidence

| Item | Test/observation | Result |
|---|---|---|
| TUI enabled idle: no child processes | `/proc/<pid>/task/*/children` after `/bridge status` | PASS (empty) |
| TUI disabled baseline: no child processes | same | PASS (empty) |
| TUI socket set equals disabled baseline | `ss -tnp -H` set equality | PASS |
| `/bridge status` answers truthfully in TUI | PTY transcript: "enabled"/"disabled" per settings | PASS (was silent no-op before the repair) |
| Shared answer unit contract | `bridge_command` 3 tests | PASS |
| Harness suite | `cargo test -p vesper-harness --features bridge --lib` | **137 passed, 0 failed** (134 + 3 new) |
| Harness feature-off | `cargo test -p vesper-harness --lib` | 116 passed (baseline unchanged) |
| Bridge core | `cargo test -p vesper-bridge` | 50 passed, 0 failed |
| ACP + bridge | `cargo test -p agent-vesper-acp --features bridge` | 84 passed, 0 failed |
| ACP feature-off | `cargo test -p agent-vesper-acp` | 83 passed, 0 failed |
| TUI + bridge | `cargo test -p agent-vesper-tui --features bridge` | 395 passed, 0 failed |
| clippy both toolchains | `-p harness/acp/tui --features …bridge… --all-targets -D warnings` (1.88.0 and stable) | 0 errors |
| fmt / architecture / naming-guard / acceptance | repo gates | clean / 28 packages / 33 frozen / 23/23 |

## Honest status

- AT-01 now holds **all three evidence classes in both hosts**:
  compiled-out (phase2), registry/advertisement (phase2), and
  OS-process/socket observation (phase2e ACP + this increment TUI).
- The TUI `/bridge` silent-drop was a real user-visible defect shipped
  in the uncommitted phase2b work; it is repaired and now guarded by the
  PTY test that exposed it. No prior report claimed TUI command
  *answering* behavior (phase2b honestly said presentation parity was
  advertisement-level), so no earlier receipt is invalidated — but the
  defect itself existed and is now fixed.
- Capture/compositor/input/adapter lanes remain unimplemented and
  uncertified; no application capability is claimed.

## Deviations

- None. The shared-module move is the BR-21-parity-correct home for the
  answer text; it was planned as "host-neutral descriptor" work and is
  now realized for the answer layer too.

## Unresolved items

1. DaVinci Resolve Studio installation (Alex) — Phase 3 unblocks.
2. Cua 0.28.1 install authorization (Alex) — Phase 4 unblocks.
3. Native enrollment freeze + verification (hosting-process restart on
   the repaired binary; this session's tool surface does not expose
   `acceptance_enroll`).

## Readiness effect

Both hosts now answer `/bridge` from one shared implementation with
kernel-accounted proof that an enabled-but-idle Bridge host leaves zero
OS footprint. Every remaining unexecuted scenario sits behind the two
blocked lanes.
