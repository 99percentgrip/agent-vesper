# Phase 3 — Native Adapter Integration (Session 2v)

**Date:** 2026-09-16 · **Status: PASS — both live routes are real `bridge_execute` tools, live-tested through the production authorization path**

## Objective

Close the gap between proven probe scripts and the product: make
`bridge_connect`/`bridge_execute` dispatch to real applications through the
full Bridge authorization chain — the PRD's "a person asks Vesper" endgame.

## What was built

### 1. `AdapterPort` seam (vesper-bridge, pure)

`adapter.rs`: `AdapterPort` trait + `AdapterOutcome`. The seam enforces the
core's honesty rules at the type level — adapters return evidence only when
independently measured; a driver ack classifies as `Applied` at best. Four
new typed `BridgeError` variants (`DependencyMissing`, `InvalidArguments`,
`Transport`, `Timeout`) carry bounded details without new I/O in the core.

### 2. Two dependency-free adapters (vesper-harness, hosted)

- **`FileIpcAdapter`** — the Resolve free-edition worker channel, exactly as
  proven in 2t: beat-freshness gate (5s), 16 KiB bound, id-correlated
  results, 20s deadline, 8 typed capabilities (`resolve.worker.*`).
- **`MprisAdapter`** — any MPRIS2 player via `busctl` (subprocess only, no
  new crates — MSRV and license posture untouched): 8 capabilities
  (`mpris.player.*`), mutations verified by **reading PlaybackStatus back**
  (typed property, not the call ack), 64 KiB output bound.
- `discover_mpris_players()` — bounded session-bus enumeration.

### 3. Service + composition wiring

- `BridgeToolService.with_adapter()` — manifest comes from the adapter;
  connect reports the live binding; execute now dispatches **after**
  authorize + revalidate, settles through the core (BR-16 duplicate
  suppression sees every effect), and reports applied/verified/failed with
  evidence lines.
- `with_bridge(true)` attaches: Resolve worker if
  `VESPER_BRIDGE_RESOLVE_IPC` is set and live → else the first live MPRIS
  player → else the truthful no-adapter composition. **Hosts never launch
  applications** (§1 boundary held).
- Real discovery: `bridge_discover` enumerates live players + adapter health.

## Bugs found and fixed this session (live-verified)

1. **busctl 4-arg form**: interface and property are separate parameters;
   my first merge produced `Too few arguments` on every property read —
   caught by the live MPRIS test, fixed, re-run green.
2. **Environment-coupled test assertions**: two tests asserted the old
   static no-adapter text and failed *because Elisa was actually running*
   (discovery truthfully reported her). Rewritten to assert state-invariants,
   not environment luck.
3. **Feature-off regression**: `bridge_command` referenced gated modules
   unconditionally — feature-off builds broke. cfg-gated fallbacks added;
   the off-state now honestly says "not compiled into this build".
4. Clippy (both toolchains): let-chains collapse, one format-inlining, a
   duplicated `#[cfg]` attribute I introduced.

## Verification receipts (all fresh)

| Gate | Result |
|---|---|
| `vesper-bridge` | **43/0** (29+10+4) |
| harness `--features bridge --lib` | **147/0**, 4 ignored (2 live-gated) |
| harness feature-off `--lib` | **122/0**, 2 ignored |
| ACP +bridge / TUI +bridge | 53/0 / 241/0 |
| clippy stable **and** 1.88.0, both crates, all targets, `-D warnings` | 0 errors |
| fmt / architecture / naming-guard / acceptance | clean / 28 pkgs / 33 frozen / **23/23** |

### Live tests (the point of this increment)

| Test | Route | Result |
|---|---|---|
| `live_mpris_player_control` (`--ignored`) | full path: connect → observe → execute → **real Elisa status read** | **PASS** |
| `live_resolve_worker_status_roundtrip` (`--ignored`, `VESPER_BRIDGE_RESOLVE_IPC` set) | full path → **real Resolve worker status**: `app=DaVinci Resolve ver=21.1.0.17 proj=test` | **PASS** |

Deterministic adapter tests (fake adapter): dispatch reaches the fake and
settles `applied`; transport failure settles `failed` with the reason;
no-adapter composition still refuses truthfully (now asserted as the honest
capability-gate denial it always was). **3/3.**

## Honest limitations

- The production adapter selection is environment-driven (env var / first
  MPRIS player); a Settings-level application picker is future work
  (matches the house "feature activation belongs in native Settings" rule).
- `job_created` exists but no adapter creates async jobs yet (render flows
  still report through `render_status`).
- Live tests require their targets running; they are `#[ignore]`d by
  default and were executed explicitly here.
- `BridgeError::Transport`-class adapter errors settle `Failed` records;
  `UnknownOutcome` reconciliation (timeout-after-mutation) is the adapter
  phase's next hardening step.

## Files

- `crates/vesper-bridge/src/adapter.rs` (new seam) + error variants + lib exports
- `crates/vesper-harness/src/bridge_adapters.rs` (new: both adapters + discovery)
- `crates/vesper-harness/src/bridge_service.rs` (adapter slot, real
  discover/connect/dispatch/settle) + adapter-test module
- `crates/vesper-harness/src/lib.rs` (`with_bridge` adapter selection),
  `bridge_command.rs` (feature-off fallbacks)
- `crates/vesper-harness/src/bridge_adapter_tests.rs` (new: 3 deterministic + 2 live)

## Verdict

**PASS.** "Ask Vesper to control an application" now runs end-to-end inside
the product tool surface, through the same authority gates the 2q audit
hardened — proven live against both a real media player and a real
professional video editor.
