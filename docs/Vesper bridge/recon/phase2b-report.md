# Vesper Bridge — Phase 2b execution report (host wiring: TUI/ACP parity)

## Objective

Complete the Phase 2 remainder: wire the Bridge surface into **both
production hosts** with one shared setting resolution, a host-neutral
`/bridge` command descriptor, and the ACP command handler — preserving
AT-01 (disabled path does nothing) and AT-34-style surface parity.

## Methods and commands

- `crates/vesper-harness/src/bridge_settings.rs` (new): explicit
  user-owned `.agent-vesper/bridge-settings.json` (JSON, deny-unknown-
  fields so a misspelled key cannot silently disable/enable), fail-closed
  load (missing/malformed ⇒ disabled), atomic save refusing symlinked
  state dirs, `holder::shared(root)` once-per-process resolution — same
  discipline as the web settings.
- `vesper_domain::slash_commands::BRIDGE_SLASH_COMMAND` (new): the
  host-neutral `/bridge` descriptor, advertised by both hosts **only**
  under their `bridge` feature.
- `apps/agent-vesper-tui`: `bridge = ["vesper-harness/bridge"]` feature;
  `with_bridge(bridge_enabled_from_settings())` in the harness
  composition; `CommandOutcome::Bridge` + `pending_bridge_command` +
  `/bridge` registration in the command surface (count assertion updated
  to `102 + swarm + bridge` with a bridge-presence assertion).
- `apps/agent-vesper-acp`: same feature; `host_parity_commands()` pushes
  `BRIDGE_SLASH_COMMAND` under the feature; `/bridge` handler routes to
  the new `bridge_host::command()` (status/discover/stop/disconnect
  answers are host-level and read-only; connect flows through the model
  tool surface, never around it).
- `HarnessToolService::with_bridge` gained a feature-off no-op variant so
  host composition code is identical with and without the feature.

Gates run (feature ON where relevant):
- `cargo test -p vesper-harness --features bridge --lib` → **131 passed**
  (was 126; +5 settings tests).
- `cargo test -p vesper-harness --lib` (feature off) → **116 passed**
  (unchanged; zero bridge code compiled).
- `cargo test -p agent-vesper-acp --features bridge` → **16/16 targets
  ok** (process-transcript advertisement assertions updated: 47+swarm+
  bridge commands, last-command order help→…→bridge).
- `cargo test -p agent-vesper-acp` (feature off) → 16/16 ok.
- `cargo test -p agent-vesper-tui` → 3/3 targets ok; `--features bridge`
  → no failures (command-surface tests include the bridge-presence
  assertion).
- `cargo clippy` over domain/bridge/harness/agent/xtask/both apps with
  `--features …bridge…` → clean at `-D warnings` (a pre-existing
  `vesper-provider-openai` test-import warning appears only under
  `--all-targets --all-features` combos outside this change's scope and
  was left untouched).
- `cargo fmt -- --check` → clean. `cargo xtask architecture` → 28
  packages. `cargo xtask naming-guard` → clean.
- `cargo xtask acceptance` → **23/23**.

## Files changed

- New: `crates/vesper-harness/src/bridge_settings.rs`,
  `apps/agent-vesper-acp/src/bridge_host.rs`.
- `crates/vesper-domain/src/slash_commands.rs` (BRIDGE_SLASH_COMMAND),
  `crates/vesper-harness/src/lib.rs` (with_bridge no-op variant),
  `apps/agent-vesper-tui/{Cargo.toml,src/commands.rs,src/dispatch.rs,
  src/main.rs}`, `apps/agent-vesper-acp/{Cargo.toml,src/lib.rs,
  tests/process_transcript.rs}`.
- Docs: this report + evidence index.

## Exact evidence

| Scenario | Test/observation | Result |
|---|---|---|
| Settings default-off & fail-closed | `missing_settings_disable_bridge`, `malformed_settings_fail_closed_to_disabled`, `unknown_fields_are_refused…` | PASS |
| Settings round-trip, isolation from web settings | `explicit_false_and_true_round_trip`, `save_does_not_touch_other_host_settings` | PASS |
| `/bridge` advertised iff feature on (both hosts) | ACP process-transcript advertisement count/order assertions; TUI command-surface registry assertion | PASS |
| Disabled build unchanged | feature-off harness lib tests (116) and both hosts' suites green without the feature | PASS |
| AT-01 core (no state when disabled) | `holder` resolves disabled; `with_bridge(false)` constructs nothing (Phase 2 test `disabled_bridge_advertises_zero_bridge_tools`) | PASS |

## Honest status

- Both hosts now share one setting resolution, one command descriptor and
  one tool surface; TUI/ACP **presentation** parity for bridge is at the
  command-advertisement level. Full AT-34 (same task through both hosts
  with equivalent approvals/results) is still **NOT TESTED** — it needs a
  real adapter and end-to-end runs.
- AT-01's process/filesystem/network observation at host startup remains
  **NOT TESTED** at the OS-observation level; the composition-level
  evidence above is what exists (no bridge code runs, nothing is
  constructed, when disabled).
- TUI `/bridge` currently surfaces status text at the idle-turn boundary
  (queued-command pattern) and does not yet render a live Bridge status
  panel; ACP answers inline. Both are presentation gaps, not authority
  gaps: no path bypasses the core gate.
- Native enrollment: still unfrozen this session (host binary predates
  the ceiling repair; retries exhaust the enrollment window during
  contract preparation). No reduced scope was enrolled.

## Deviations

- The TUI's `/bridge` handling reuses the swarm queued-command pattern;
  a dedicated panel is deferred to Phase 3+ UX work.
- `/bridge stop` at host level is informational; the authoritative stop
  is the session stop path in the core (NF-02) — surfaced truthfully in
  the command text rather than pretending a host string stops work.

## Unresolved items

1. Native enrollment freeze + verification (needs hosting-process restart
   on the repaired binary).
2. MSRV 1.88 confirmation for the new feature combination.
3. Live host-startup OS observation for AT-01 (process/network traces).
4. Phase 3 (Resolve vertical slice) remains BLOCKED on an installation;
   Phase 4 (Cua lane) on driver authorization.

## Readiness effect

Phase 2's host-wiring gap is closed at the composition and
command-surface level with both hosts building and testing green in
feature-on and feature-off configurations. The next permitted step is
either the host-startup AT-01 observation evidence or Phase 3 once
Resolve exists.
