# vesper-agent — Tier C agent loop and tool execution

## Purpose

Own the multi-turn, tool-executing agent loop that composes `vesper-runtime`'s
single-turn provider dispatch into a ReAct loop (ADR 0010). Provide the tool
registry, the `ToolExecutor` contract, the permission gate, and the loop
mechanics. The runtime stays provider-neutral and single-turn; this crate is
the multi-turn, tool-executing layer above it.

## Ownership

- `src/executor.rs` — `ToolExecutor`/`ToolService` traits, hosted subsystem
  adapters, `ToolContext`, `ToolResult`, `ToolError`, and the schema-definition
  helper.
- `src/confinement.rs` — path-confinement *enforcement* (`confine`). `vesper-security`
  ships only authority descriptors, so the executor layer owns the
  canonicalize/boundary enforcement here (symlinks followed; escapes fail closed).
- `src/tools.rs` — the nine core parity-critical **real** executors (`read_file`,
  `write_file`, `edit_file`, `apply_patch`, `list_directory`, `search_files`,
  `grep`, `run_command`, `update_plan`) with confined filesystem/shell I/O.
- `src/registry.rs` — `ToolRegistry`: name → executor routing + mode-filtered
  `definitions_for`.
- `src/permission.rs` — pure `check_tool_permission(mode, permission, class)`
  plus the host-owned asynchronous `PermissionPort`; `Ask` never authorizes
  by itself and the default port fails closed.
- `src/project_context.rs` — bounded, symlink-safe progressive discovery of
  project instruction files with secret-assignment redaction.
- `src/agent_loop.rs` — `AgentLoop::run_prompt` and
  `AgentLoop::run_prompt_with_history`: dispatch turn → collect tool calls →
  gate → execute → append `role: Tool` results → repeat, bounded by
  `max_tool_iterations`. Captures `update_plan` output into
  `AgentTurnOutcome::plan` so callers drive the Phase 5 PLANNING → REVIEW
  transition. `AgentProgressPort` emits bounded in-memory provider/tool/plan
  activity without tool arguments, outputs, paths, or secrets; hosts may also
  clone a loop with per-turn provider/model configuration.

## Local Contracts

- Compose `vesper-runtime::ProviderRegistry` for turn dispatch; do NOT add
  multi-turn state or tool execution to the runtime itself.
- Hosts own conversation history and may inject memory, MCP, plugin, worker,
  or automation tools through `ToolService`; the core loop remains unaware of
  those concrete subsystems.
- Hosts should populate `AgentLoopConfig.system_instructions` at their
  composition boundary with `project_instructions`; the helper is bounded and
  does not persist or mutate project files.
- `ToolContext` carries the current visible conversation for context-aware
  hosted tools. Provider requests use a deterministic bounded window of at
  most `MAX_CONTEXT_MESSAGES` messages while the host retains the returned
  transcript.
- Depends on `vesper-domain`, `vesper-provider`, `vesper-runtime` (+ `glob`,
  `regex` for search; `tempfile` dev-only). Must NOT depend on `vesper-acp`,
  `vesper-sessions`, SQLite, MCP, frontends, or any disposable spike.
- Every path-bearing tool routes its argument through `confinement::confine`
  against the session's primary workspace root before any I/O. `run_command`
  runs in the workspace root via the platform shell (`sh -c` / `cmd /C`) with a
  bounded timeout; its grandchild risk is documented (no safe `killpg` under
  `#![forbid(unsafe_code)]`).
- `update_plan` writes only `.agent/plan.md` (confined) and returns the rendered
  markdown so the loop surfaces it for the TUI REVIEW transition.
- The permission gate is the single authority checkpoint before any executor
  runs; `ReadOnly` tools always pass, `Mutating`/`Shell`/`Process`/
  `NestedWorkflow` require `Code` mode, `Bypass`, or a host-approved `Ask`
  decision. `Ask` without a `PermissionPort` fails closed.
- `#![forbid(unsafe_code)]`, workspace MSRV 1.88, workspace lints, and
  `-D warnings` Clippy apply.

## Work Guidance

- The loop is bounded by `max_tool_iterations`; every iteration is exactly one
  `ProviderSession::start`. Multi-turn conversation state is supplied and
  returned by the host through `run_prompt_with_history`; it never lives in
  provider session state.
- Permission denials and unknown/failed tools are fed back to the model as
  bounded `role: Tool` text so the turn can recover (mirrors the oracle).
- When adding a tool: add the executor in `tools.rs`, register it in
  `ToolRegistry::parity_default` when it is provider-neutral core behavior.
  Host-owned memory, checkpoint, MCP, plugin, worker, and automation tools
  are injected through `ToolService::with_service`; set their
  `ToolExecutionClass` in the host definition and add a mode-eligibility
  test for the composition boundary.

## Verification

- Run `cargo test -p vesper-agent`.
- Run `cargo xtask verify` (fmt + clippy + workspace tests + architecture).
- `cargo run --package xtask --quiet -- architecture` must include
  `vesper-agent` and validate its dependency direction.

## Child DOX Index

No children.
