# Vesper Bridge — Increment 4: MSRV sweep, deadlock repair, gate receipts

## Objective

Close the three open items from phase2b/2c that required no blocked lane:
MSRV 1.88 verification across the bridge feature combination, honest
receipts for the current uncommitted tree, and a bug found while
re-validating the Phase 2 evidence (a self-deadlock in the hosted Bridge
service).

## Methods and commands

- `cargo +1.88.0 check` then `cargo +1.88.0 clippy --all-targets` for
  `vesper-bridge`, `vesper-harness --features bridge`, both apps with
  `bridge`, then a full `--workspace --all-targets --all-features` sweep
  on 1.88.0.
- Root-caused a hang with `gdb -p <pid> -batch -ex "thread apply all bt"`
  on the stuck test process; stack showed
  `replay_observation` → `next_observation` re-locking the same
  non-reentrant `std::sync::Mutex`.
- Fixed the deadlock, then preserved denial precedence when the missing
  observation had to remain a BR-15 stale denial instead of a synthetic
  fresh observation.
- Re-ran: feature-off harness lib, feature-on harness lib (single-threaded
  and default-threaded), `vesper-bridge`, ACP with bridge, fmt (both
  toolchains), clippy (both toolchains), architecture, naming-guard,
  `cargo xtask acceptance`.

## Files changed

- `crates/vesper-harness/src/bridge_service.rs` — `replay_observation`
  re-lock deadlock repaired; missing-observation handling now returns an
  explicitly **degraded, zero-revision** observation so `authorize`
  refuses it at the freshness gate *after* mode/permission/capability
  checks (denial precedence preserved: steps 3→5 of `authorize`).
- `crates/vesper-bridge/src/session.rs` — `reconcile()` borrow bug fixed
  (the `drop(record)` no-op warning on MSRV; quarantine path now returns
  `UnknownOutcome` without re-borrowing the map).
- MSRV-1.88 clippy mechanical fixes (pre-existing `uninlined_format_args`
  / `expect_fun_call` findings that only that toolchain's clippy flags):
  `vesper-sandbox` lib + `sandbox_init` bin, `vesper-agent` VRO modules
  and two test files, `vesper-memory` two test files, `vesper-swarm`
  governance/orchestrator, `apps/agent-vesper-tui/src/main.rs` MCP/embedding
  status strings.

## Exact evidence

| Item | Command | Result |
|---|---|---|
| Bridge crate MSRV | `cargo +1.88.0 check -p vesper-bridge --tests` | PASS (0 errors, 0 warnings) |
| Harness+bridge MSRV | `cargo +1.88.0 check -p vesper-harness --features bridge --tests` | PASS |
| ACP+bridge MSRV | `cargo +1.88.0 check -p agent-vesper-acp --features bridge --tests` | PASS |
| TUI+bridge MSRV | `cargo +1.88.0 check -p agent-vesper-tui --features bridge` | PASS |
| Workspace MSRV clippy | `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings` | PASS (0 errors) |
| Stable clippy (bridge combo) | `cargo clippy -p vesper-harness -p vesper-bridge --features vesper-harness/bridge --all-targets -- -D warnings` | PASS |
| Deadlock repair | `cargo test -p vesper-harness --features bridge --lib` (default threads) | **134 passed, 0 failed** in 10.02 s (previously hung indefinitely on 5 tests) |
| Feature-off isolation | `cargo test -p vesper-harness --lib` | 116 passed (baseline unchanged) |
| Bridge core suite | `cargo test -p vesper-bridge` | 53 tests passed across targets |
| ACP with bridge | `cargo test -p agent-vesper-acp --features agent-vesper-acp/bridge` | all targets ok |
| fmt | `cargo fmt --all -- --check` | PASS |
| architecture | `cargo xtask architecture` | 28 packages validated |
| naming-guard | `cargo xtask naming-guard` | clean (33 frozen) |
| acceptance gate | `cargo xtask acceptance` | 23/23 exact cases passed |

## Deadlock finding (corrected receipt)

The Phase 2b report's "**131 passed**" harness receipt is **not
reproducible on this tree**: `replay_observation()` held the
`last_observation` std Mutex across a call to `next_observation()`, which
locks the same Mutex — a self-deadlock triggered by `bridge_execute`
after `bridge_connect` without an intervening `bridge_observe` (5 tests:
3 service-route denials, 2 AT-21 stop tests). gdb stack capture pinned
it to `bridge_service.rs:172/184`. The prior receipt was recorded from a
run that must have differed from the current source (exact provenance
unknowable now); the corrected receipt is the table above.

The same repair removed a **second** latent bug: `bridge_execute` with no
prior observation used to mint a synthetic fresh observation, silently
binding preconditions to state that was never observed — masking the
BR-15 stale-precondition contract and the AT-21
resume-requires-fresh-observation invariant. It now yields a degraded
zero-revision observation that `authorize` refuses at the freshness gate,
with denial precedence preserved (mode/permission/capability denials
still rank ahead of the freshness denial, per `authorize`'s documented
step order and the phase2 gate-order repair).

## Honest status

- No acceptance scenario is claimed beyond Phase 2's core-route evidence;
  the deadlock repair strengthens AT-06/AT-07-style denial routing and
  the AT-21 freshness invariant but is not live-application evidence.
- Native enrollment remains unavailable to this session (the harness's
  `acceptance_enroll` tool is not exposed on the session tool surface;
  the installed hosting binary also predates the 512-paragraph ceiling
  repair). No reduced scope was enrolled; no completion is claimed.
- Phase 3 (Resolve) and Phase 4 (Cua) lanes remain BLOCKED on Alex's
  manual actions per the compatibility manifest.

## Deviations

- MSRV clippy fixes touched files outside Bridge (sandbox/agent/memory/
  swarm/TUI). They are mechanical lint compliance only — no behavior
  change — and were required because the workspace-wide 1.88 gate fails
  on any crate that lints. No broad dependency or API change was made.

## Unresolved items

1. DaVinci Resolve Studio installation (Alex) — Phase 3 unblocks.
2. Cua 0.28.1 install authorization (Alex) — Phase 4 unblocks.
3. Native enrollment freeze + verification — needs the hosting process
   restarted on the repaired binary; this session's tool surface does not
   expose `acceptance_enroll`.
4. Live host-startup OS observation for AT-01 (process/network traces).

## Readiness effect

The MSRV lane is closed for the bridge combination; every repository
gate is green on the current tree with reproducible receipts; and the
Bridge service no longer contains a hang that would have frozen any real
`connect → execute` flow without a prior observation — the exact
first-use path of the PRD's user story.
