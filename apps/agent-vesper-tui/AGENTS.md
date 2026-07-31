# Agent Vesper TUI

## Purpose

Own the Stage 11b Terminal User Interface: provider-superpowers discovery,
the Plan Mode state machine, the slash-command registry, the
`TerminalRenderer` abstraction, and the `ratatui` + `crossterm` event loop.
The crate is a thin composition boundary that queries the runtime registry
for superpowers at startup and renders the active provider's controls
natively; it owns no provider-wire, ACP, persistence, or session-mutation
business logic.

## Ownership

- `src/plan_mode.rs` — pure 4-phase Plan Mode state machine
  (NORMAL → PLANNING → REVIEW → EXECUTING) mirroring the Python oracle's
  `PLAN_MODE_PROMPT`.
- `src/commands.rs` — slash-command parsing, registry, and resolution
  against the active provider's superpowers.
- `src/superpowers.rs` — `ProviderSuperpowerSurface` and
  `SuperpowerOverrides`, the pure projection the TUI keeps of the active
  provider's advertised descriptors.
- `src/ui.rs` — `TerminalRenderer` trait, `ViewModel`, `StubRenderer` for
  tests, and the production `render_to_frame` ratatui/crossterm backend.
- `src/lib.rs` — public re-exports and `query_startup_view`, the single
  integration point between the TUI and the runtime registry.
- `src/main.rs` — binary entry point; crossterm raw-mode + alternate-screen
  lifecycle and the interactive event loop.

## Local Contracts

- Stdout carries only terminal escapes via crossterm; no ACP/JSON-RPC may
  appear there. Tracing goes to stderr only.
- The crate depends on `vesper-domain`, `vesper-provider`,
  `vesper-provider-glm`, `vesper-provider-synthetic`, and `vesper-runtime`;
  it must not depend on `vesper-acp`, `vesper-sessions`, SQLite, MCP, or
  any disposable spike.
- The Plan Mode state machine is **pure**: no I/O, no async, no global
  state. Every transition returns a `PlanTransition`; the event loop applies
  it.
- Plan Mode reasoning text is produced by the model through the runtime; the
  TUI owns the transition discipline, not the reasoning.
- The crate stays `#![forbid(unsafe_code)]` and respects workspace MSRV
  1.88, workspace lints, and `-D warnings` Clippy.
- Superpower commands (`/effort`, `/thinking`, `/model`) are resolved
  dynamically against the active provider's advertised descriptors at
  dispatch time, so the same command surface works for any registered
  provider.
- Provider selection follows `AGENT_VESPER_PROVIDER` (default `zai`), the
  same composition-boundary convention as `agent-vesper-acp`.

## Work Guidance

- Keep the Plan Mode, command registry, superpower adapter, and renderer
  trait unit-testable without touching a real terminal — the production
  binary is the only module that may invoke crossterm directly.
- When adding a new slash command, register it in
  `CommandRegistry::stage_11b`, document its surface in
  `CommandRegistry::help_text`, and add a test that proves it resolves
  correctly across phases.
- When adding a new provider superpower, declare it in the provider's
  factory (e.g. `glm_superpowers` in `vesper-provider-glm::factory`); the
  TUI surfaces it automatically once the provider is registered with
  `register_with_superpowers`.

## Verification

- Run `cargo test -p agent-vesper-tui --lib`.
- Run `cargo clippy -p agent-vesper-tui --all-targets --all-features -- -D warnings`.
- Run `cargo run --package xtask --quiet -- architecture` (the TUI must
  appear in the validated package count and pass the dependency-direction
  gate).
- Run `cargo build -p agent-vesper-tui --bins` to confirm the binary
  links under the workspace toolchain.

## Supply-chain note

The crate pins `ratatui = "=0.30.2"` specifically because ratatui 0.29.0
pulled in `paste 1.0.15` (RUSTSEC-2024-0436 — unmaintained) and `lru
0.12.5` (RUSTSEC-2026-0002 — unsound `IterMut`). ratatui 0.30.2 dropped
the `paste` dependency entirely and moved to `lru 0.18.1`, so both
advisories are eliminated without ignoring them. Do not downgrade
ratatui without re-checking `cargo deny check` and `cargo audit`.

## Child DOX Index

No children.
