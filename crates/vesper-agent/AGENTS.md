# vesper-agent — Tier C agent loop and tool execution

## Purpose

Own the multi-turn, tool-executing agent loop that composes `vesper-runtime`'s
single-turn provider dispatch into a ReAct loop (ADR 0010). Provide the tool
registry, the `ToolExecutor` contract, the permission gate, and the loop
mechanics. The runtime stays provider-neutral and single-turn; this crate is
the multi-turn, tool-executing layer above it.

## Ownership

- `src/executor.rs` — `ToolExecutor` trait, `ToolContext`, `ToolResult`,
  `ToolError`, and the schema-definition helper.
- `src/tools.rs` — the nine parity-critical stub executors (`read_file`,
  `write_file`, `edit_file`, `apply_patch`, `list_directory`, `search_files`,
  `grep`, `run_command`, `update_plan`) advertising their `ToolExecutionClass`.
- `src/registry.rs` — `ToolRegistry`: name → executor routing + mode-filtered
  `definitions_for`.
- `src/permission.rs` — pure `check_tool_permission(mode, permission, class)`.
- `src/agent_loop.rs` — `AgentLoop::run_prompt`: dispatch turn → collect tool
  calls → gate → execute → append `role: Tool` results → repeat, bounded by
  `max_tool_iterations`.

## Local Contracts

- Compose `vesper-runtime::ProviderRegistry` for turn dispatch; do NOT add
  multi-turn state or tool execution to the runtime itself.
- Depends on `vesper-domain`, `vesper-provider`, `vesper-runtime`. May add
  `vesper-policy`/`vesper-security` when real executors need them. Must NOT
  depend on `vesper-acp`, `vesper-sessions`, SQLite, MCP, frontends, or any
  disposable spike.
- Phase 1 ships stub executors (canned results, no I/O). Real filesystem/shell
  execution arrives in Phase 4 behind the same `ToolExecutor` contract, with
  path confinement via `vesper-security`.
- The permission gate is the single authority checkpoint before any executor
  runs; `ReadOnly` tools always pass, `Mutating`/`Shell`/`Process`/
  `NestedWorkflow` require `Code` mode + non-`ReadOnly` permission.
- `#![forbid(unsafe_code)]`, workspace MSRV 1.88, workspace lints, and
  `-D warnings` Clippy apply.

## Work Guidance

- The loop is bounded by `max_tool_iterations`; every iteration is exactly one
  `ProviderSession::start`. Multi-turn conversation state lives in the
  `messages` list the loop owns — never in provider session state.
- Permission denials and unknown/failed tools are fed back to the model as
  bounded `role: Tool` text so the turn can recover (mirrors the oracle).
- When adding a tool: add the executor in `tools.rs`, register it in
  `ToolRegistry::parity_default`, set its `ToolExecutionClass`, and add a
  registry/permission test for its mode eligibility.

## Verification

- Run `cargo test -p vesper-agent`.
- Run `cargo xtask verify` (fmt + clippy + workspace tests + architecture).
- `cargo run --package xtask --quiet -- architecture` must include
  `vesper-agent` and validate its dependency direction.

## Child DOX Index

No children.
