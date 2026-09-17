# Vesper Bridge — Phase 2 execution report (harness composition, default-off)

## Objective

Implement the VB-PRD-001 Phase 2 core composition: expose the Bridge tool
surface through the existing `vesper-harness` hosted-service seam behind a
**default-off** `bridge` feature, with advertisement gating, honest
no-adapter behavior, and mode/permission routing through the pure core's
gate — while the disabled build stays byte-identical in behavior (BR-30,
NF-01, AT-01).

## Methods and commands

- `crates/vesper-harness/src/bridge_service.rs`: `BridgeToolService`
  (single session owner), the 8-tool surface
  (`bridge_discover/connect/capabilities/observe/execute/job_status/
  verify/disconnect`), and denial→text mapping (§13.2 language).
- `HarnessToolService`: new `bridge: Option<Arc<BridgeToolService>>` field
  (`#[cfg(feature = "bridge")]`), `with_bridge(bool)` builder (mirrors
  `with_web_scope`), definition extension and execution routing placed
  before the web-tools route.
- Tests: `bridge_service_tests.rs` (10 integration-class tests, run only
  under `--features bridge`).
- Gates run:
  - `cargo test -p vesper-harness --features bridge --lib` → **126 passed,
    0 failed** (was 116; +10 bridge tests).
  - `cargo test -p vesper-harness --lib` (feature OFF) → **116 passed** —
    the default build is unchanged and every bridge test is compiled out.
  - `cargo test -p vesper-bridge` → 46 passed (29+17).
  - `cargo clippy -p vesper-harness -p vesper-bridge -p xtask
    --all-targets --features bridge -- -D warnings` → clean.
  - `cargo fmt` → clean.
  - `cargo xtask architecture` → **28 packages** (allowlist grew by the
    harness→bridge optional edge with rationale).
  - `cargo xtask naming-guard` → clean (33 frozen).
  - `cargo xtask acceptance` → **23/23** (unchanged by the change).

## Files changed

- `crates/vesper-harness/Cargo.toml` — `bridge = ["dep:vesper-bridge"]`
  default-off feature; optional dependency.
- `crates/vesper-harness/src/lib.rs` — field, builder, definition
  extension, execution route, cfg'd test-fixture field.
- `crates/vesper-harness/src/bridge_service.rs` — new.
- `crates/vesper-harness/src/bridge_service_tests.rs` — new.
- `crates/vesper-bridge/src/session.rs` — gate-order repair: mode/permission
  enforcement now precedes capability-resolution diagnostics so a denied
  mutation reports the denial, not an incidental capability error
  (denial precedence, BR-07/08). 46 bridge tests still green.
- `xtask/src/main.rs` — harness allowlist entry.
- Docs: this report; evidence index.

## Exact evidence

| Scenario | Test | Result |
|---|---|---|
| AT-01/BR-30 disabled ⇒ zero bridge tools advertised | `disabled_bridge_advertises_zero_bridge_tools` | PASS |
| BR-30 disabled ⇒ direct tool call refused, never executed | `disabled_bridge_tool_call_is_refused_not_executed` | PASS |
| Enabled ⇒ exactly 8 tools with honest classes | `enabled_bridge_advertises_exactly_eight_tools` + `eight_tools_are_advertised_with_honest_classes` | PASS |
| No-adapter composition refuses execute truthfully (no fabricated effect) | `no_adapter_composition_refuses_execute_truthfully` | PASS |
| AT-06 plan mode ⇒ denial names plan mode | `plan_mode_mutation_is_denied_through_the_service_route` | PASS |
| BR-07 read-only permission ⇒ denial names read-only | `read_only_permission_mutation_is_denied_through_the_service_route` | PASS |
| BR-01 passive discover, truthful no-adapter | `discover_is_passive_and_names_no_adapter_truthfully` | PASS |
| Session-before-execute | `execute_requires_session_first` | PASS |
| NF-01 build-path isolation | feature-off lib test run (116, no bridge code compiled) | PASS |

## Honest status

- Phase 2 is **partially complete**: the harness composition and its
  deterministic evidence exist, but TUI/ACP host wiring (settings toggle,
  `/bridge` commands, stop key, progress), consent surfaces and
  long-lived-connection supervision are **not implemented**. AT-01's full
  claim ("both host startup paths, process/filesystem/network
  observation") is therefore **NOT TESTED** at host level; only the
  registry/advertisement and direct-call paths are proven.
- All 44 AT scenarios remain formally **NOT TESTED**; the AT-01/AT-06 rows
  have partial core-route evidence recorded above, not full acceptance.
- No adapter, driver, capture, network or process spawn was added —
  `bridge_execute` on the no-adapter composition explicitly refuses
  rather than fabricating an application effect.
- Native enrollment remains unfrozen this session (hosting binary predates
  the ceiling repair; retries hit the enrollment window). No reduced
  scope was enrolled; no completion is claimed.

## Deviations

- `bridge_verify` returns an error in this phase (there is never an
  applied operation to verify without an adapter) — a truthful refusal,
  not a stub success.
- Host-level feature toggle reading `.agent-vesper/config.toml` is left to
  the hosts' composition step (Phase 2 remainder), so `with_bridge` takes
  a bool for now.

## Unresolved items

1. TUI/ACP wiring: settings toggle, `/bridge` command surface, stop path,
   progress lines, AT-34 parity tests.
2. Native enrollment freeze + verification (host restart on repaired
   binary).
3. MSRV 1.88 confirmation for `vesper-bridge`/`vesper-harness --features
   bridge`.
4. Phases 3/4 remain blocked (no Resolve; no Cua authorization).

## Readiness effect

The default-off contract is proven in both directions (zero surface when
disabled; honest refusal when enabled without an adapter). The next
permitted step is host wiring, then the Resolve lane once an installation
exists.
