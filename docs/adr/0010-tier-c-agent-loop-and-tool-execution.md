# ADR 0010: Tier C — Agent Loop, Tool Execution, and Model-Driven Planning

Status: ACCEPTED

Supersedes: [ADR 0009](0009-provider-parity-reasoning-dial-and-planning-deferment.md) Decision 2.

## Context

ADR 0009 deferred model-driven plan generation because the Rust runtime is, by
contract, single-turn and tool-free. The lead architect has **lifted the
single-turn, tool-free restriction at the workspace level** to achieve true
functional parity with the frozen Python oracle
(`/home/alex/Projects/Native GLM-5.2 Provider` @ `bf4d4287`).

### Oracle tool surface (audit)

The oracle exposes the model to a `TOOL_DEFINITIONS` registry
(`glm_acp/tools.py:205`) and runs a ReAct loop (`agent.py:2837`
`run_loop`: *stream → tool calls → execute → feed `role: tool` results →
repeat*), bounded by `max_tool_iterations` (default 50, max 1000, persisted).

**Core Tier-C tools (the parity-critical subset):**

| Tool | Schema (required) | `ToolExecutionClass` | Purpose |
|---|---|---|---|
| `read_file` | `path` (+ `start_line`/`end_line`) | `ReadOnly` | Read file contents |
| `list_directory` | `path?` | `ReadOnly` | List a directory |
| `search_files` | `pattern` (+ `path?`) | `ReadOnly` | Glob file search |
| `grep` | `pattern` (+ `path?`, `include?`) | `ReadOnly` | Regex content search |
| `write_file` | `path`, `content` | `Mutating` | Create/overwrite a file |
| `edit_file` | `path`, `old_text`, `new_text` | `Mutating` | Exact-block replace |
| `apply_patch` | `path`, `patch` | `Mutating` | Unified diff, atomic |
| `run_command` | `command` (+ `timeout?`) | `Shell` | Build/test/git execution |
| `update_plan` | `tasks: [{content, status, priority?}]` | `ReadOnly` (no FS mutation outside `.agent/`) | Plan tracker → REVIEW |

**Permission gating (oracle):** `DESTRUCTIVE_TOOLS` frozenset
(`config.py:639`) = `{write_file, edit_file, apply_patch, apply_patch_set,
run_command, store_memory, ..., delegate_task, worktree_worker}`. Tool
eligibility is mode-based (`agent.py:2843`):
- `code` mode → ALL tools.
- `plan`/`ask` mode → read-only subset (`read_file`, `list_directory`,
  `search_files`, `grep`, `recall_*`, `session_search`, `list_skills`,
  `read_skill`, `update_plan`, `update_awareness`, `update_deliberation`).

**`update_plan` → review flow:** the model emits `update_plan` tool calls
(`agent.py:3537`); `_handle_update_plan` (`agent.py:4618`) sanitizes entries
and `_send_plan` (`agent.py:4748`) pushes an ACP `update_plan` session update
that the editor renders as a checklist.

### Rust foundation already present

The provider-neutral layer already carries the primitives Tier C needs — they
do **not** need to be invented:
- `ToolDefinition` with `execution_class: ToolExecutionClass`
  (`ReadOnly | Mutating | Shell | Process | NestedWorkflow`) —
  `crates/vesper-domain/src/tool.rs:124`. **This is the permission taxonomy.**
- `ToolCall { id, tool_id, arguments }` — `tool.rs:144`.
- `ProviderRequest.tools: Vec<ToolDefinition>` + `tool_choice`.
- Streaming `ProviderStreamEvent::ToolCallStarted/Delta/Completed(ToolCall)`.
- `SessionOperatingMode` + `SessionPermissionMode` (stored on `SessionSnapshot`,
  not yet enforced).

**Gap:** `vesper-runtime`'s `run_turn` terminates the turn on
`FinishOutcome::ToolCalls` (`supervisor.rs:1262`) without executing or looping.
`SessionTurnResult` does not surface the completed tool calls.

## Decision

**Authorize Tier C via a new `vesper-agent` crate** that composes
`vesper-runtime` (the single-turn provider engine) into a multi-turn,
tool-executing agent loop. The runtime's tool-free contract is **preserved** —
the loop and executors live above the runtime, not inside it.

This honors the directive ("lift the restriction") at the right architectural
layer: the workspace gains tool execution; the pure single-turn engine stays
provider-neutral and independently testable.

## The Tier C Blueprint (step-by-step)

### Phase 1 — Tool Registry + Executor trait (`vesper-agent`)

- New crate `crates/vesper-agent`, `#![forbid(unsafe_code)]`, depends on
  `vesper-runtime`, `vesper-domain`, `vesper-provider`, `vesper-policy`.
- `pub trait ToolExecutor: Send + Sync` — `async fn execute(&self, call:
  &ToolCall, ctx: &ToolContext) -> ToolResult` returning bounded output +
  fallback decisions. One impl per tool.
- `pub struct ToolRegistry` — maps `HarnessToolName → ToolExecutor` +
  `ToolDefinition`; exposes `definitions_for(mode: SessionOperatingMode) ->
  Vec<ToolDefinition>` (mode-filtered, mirroring `agent.py:2843`) and
  `execute(call, ctx)`.
- `ToolContext` carries the workspace root(s), permission mode, cancellation,
  and the iteration budget — never secrets.

### Phase 2 — The Execution Loop (`vesper-agent::AgentLoop`)

- `pub struct AgentLoop { supervisor, registry }`.
- `async fn run_prompt(&self, session_id, content) -> AgentTurnResult`:
  1. Dispatch one provider turn via the runtime supervisor (tools =
     `registry.definitions_for(mode)`).
  2. Collect completed `ToolCall`s from the turn (see Phase 2b).
  3. For each call: gate by permission (Phase 3) → execute via registry →
     append a `ConversationMessage` of role `Tool` with the result.
  4. If any tool ran and the iteration budget remains, loop to (1); else
     terminate.
- **2b. Surface tool calls:** extend `SessionTurnResult` with
  `tool_calls: Vec<ToolCall>` (populated from `ToolCallCompleted` events the
  supervisor already emits). This is a runtime extension, not a contract
  breach — it only *reports* what the provider returned; execution stays in
  `vesper-agent`.
- **Loop guards** (mirror the oracle): `max_tool_iterations` cap, repeated-
  batch detection, failed-command guard, unverified-change guard.

### Phase 3 — Permission gating (`vesper-agent` + `vesper-policy`)

- Map `SessionOperatingMode × SessionPermissionMode × ToolExecutionClass` to
  allow/deny/defer in `vesper-policy` (pure decisions; no I/O).
- `ReadOnly` tools always allowed. `Mutating`/`Shell`/`Process`/`NestedWorkflow`
  require `code` mode + permission: `Read`/`ask` denies; `Bypass` allows;
  `Ask` defers to a runtime permission request (new `HarnessCommandPayload`
  variant `ProvidePermissionDecision` already exists in the domain).
- **Path confinement:** every tool with a `path`/`pattern` argument is
  confined to the session workspace root(s) via `vesper-security`'s
  confinement primitives — escapes return a sanitized error, never execute.
- `update_plan` writes are confined to `.agent/plan.md` only (its execution
  class is `ReadOnly` for the FS, with a single sanctioned write path).

### Phase 4 — Core tool implementations (`vesper-agent::tools`)

Build the nine parity-critical executors in dependency order:
1. `read_file`, `list_directory`, `search_files`, `grep` (read-only;
   `vesper-security` path confinement + bounded output).
2. `write_file`, `edit_file`, `apply_patch` (mutating; confined; atomic for
   patch).
3. `run_command` (shell; confined to workspace; bounded timeout; no secret
   env leakage).
4. `update_plan` (plan-state mutation → emits a plan update, see Phase 5).

Each executor returns bounded `ToolResult` content; never raw secrets.

### Phase 5 — `update_plan` → TUI REVIEW wiring

- `update_plan` execution writes the sanitized task list to session state
  (the `ReplayPlan` the runtime already carries) and emits a live plan-update
  event through the same channel `vesper-acp` uses for persisted replay
  (`adapter.rs:563-572` is extended to also emit from a live turn).
- The TUI subscribes: when a plan arrives during `PLANNING`, it transitions
  `PLANNING → REVIEW` automatically (replacing the manual `/review <body>`
  placeholder) and renders the model-authored plan body for `/approve`.
- `/approve` (REVIEW → EXECUTING) is unchanged; it gates Phase-4 mutating
  tools on.

### Phase 6 — TUI binary ↔ `AgentLoop` end-to-end wiring (COMPLETED)

**Status: shipped.** Connects the brain (model), the hands (tools), and the
steering wheel (TUI) into a single end-to-end loop, achieving 100% functional
parity with the Python oracle's `run_loop`.

- `apps/agent-vesper-tui/src/main.rs` now instantiates
  `vesper_agent::AgentLoop` over the same shared `ProviderRegistry` that
  backs the reasoning-override supervisor. Construction
  (`build_agent_loop` / `build_agent_config`) is provider-aware (GLM `zai` /
  `vesper-synthetic`) and credential-free; only `run_prompt` dispatches.
- Free-text prompts submitted in NORMAL phase spawn the loop in a background
  `tokio::spawn`. The event loop `try_recv`s the result each iteration so
  the UI stays non-blocking; a "WORKING..." status banner is shown in-flight
  and clears the moment the result lands.
- The decisive bridge: when the loop returns
  `AgentTurnOutcome::Completed { plan: Some(body), .. }`, the binary routes
  the model-authored body through `dispatch::apply_model_plan` to drive
  `PLANNING → REVIEW`. PLANNING-phase free text stays inline (the driver
  answers the pending question); the loop is never spawned there.
- Architecture: `agent-vesper-tui` now depends on `vesper-agent`
  (registered in `xtask::allowed_dependencies`). The 15-package arch gate
  stays green.
- Verification: 12 binary tests cover provider resolution, `AgentLoop`
  construction, the `AgentEvent → SessionState` mapper, and the
  spawn/drain plumbing. `cargo xtask verify` runs clean.

### Phase 7 — Command migration (the ~75 deferred commands)

The oracle's ~80 slash commands break into three classes; migrate in this order:
- **Tool-backed, parity-critical (Tier C scope):** `/plan`, `/approve`,
  `/cancel`, `/mode`, `/clear-plan` (plan lifecycle); `/model`, `/thinking`
  (superpowers, already wired).
- **Tool-backed, later phase:** `/diff`, `/undo`, `/rollback`, `/checkpoint`,
  `/security-review`, `/recap`, `/context`, `/tasks`, `/status` — implement
  after Phase 4 executors exist; each maps to read-only tools + presentation.
- **Out of Tier C (separate stages):** `/mcp`, `/plugins`, `/mobile`,
  `/sound`, `/voice`, `/sessions-new` (worktrees), `/release`, `/loop`,
  `/insights`, cron — depend on subsystems that don't exist; defer with a
  recorded gap list.

## Alternatives considered

- **Add the loop directly to `vesper-runtime`:** rejected — it would violate
  the runtime's provider-neutral, single-turn contract and entangle provider
  dispatch with filesystem/process authority. `vesper-agent` keeps the layers
  clean and the runtime independently testable.
- **Per-provider agent loop:** rejected — the loop is provider-neutral (it
  drives whatever provider the runtime dispatches to); only the tool surface
  is workspace-local.
- **Skip the `ToolExecutionClass` taxonomy and use a flat allow-list:**
  rejected — the existing `ReadOnly|Mutating|Shell|Process|NestedWorkflow`
  taxonomy already encodes the oracle's `DESTRUCTIVE_TOOLS` + mode gating at
  the type level, enabling compile-time-visible authority.

## Consequences

- New `vesper-agent` crate owns the loop, executors, and permission
  enforcement; `vesper-runtime` stays pure.
- `SessionTurnResult` gains a `tool_calls` field (additive; existing tests
  unaffected — empty for tool-free providers).
- `vesper-acp` gains a live `update_plan` emit path (extends the existing
  replay-only channel).
- The TUI `/review <body>` placeholder is retired in favor of the
  model-driven `update_plan` flow.

## Security implications

- `#![forbid(unsafe_code)]` is preserved everywhere; no `unsafe` is introduced.
- All path-bearing tools are confined to workspace roots via `vesper-security`;
  escapes fail closed.
- `run_command` is confined, timeout-bounded, and its environment is scrubbed
  of secret references before execution.
- Permission decisions are pure (`vesper-policy`) and observable; `Ask` mode
  defers destructive ops to an explicit user decision (no silent mutation).
- `update_plan`'s only filesystem write is `.agent/plan.md`.

## Migration implications

- Tier C is additive: the default provider turn path is unchanged; the agent
  loop is an opt-in composition (the ACP/TUI binaries adopt it after the loop
  + read-only tools pass their gates). Existing single-turn tests remain green.
- The GLM adapter already serializes `tools` and parses tool-call streams
  (`vesper-provider-glm`); no provider change is required for Tier C.

## Verification requirements

- `cargo test --workspace --all-features` and `cargo xtask verify` stay green.
- New gates (in `vesper-agent`):
  - Path-confinement negative tests: every tool rejects paths escaping the
    workspace root.
  - Permission-gating matrix tests: each `(mode, permission, execution_class)`
    triple resolves to allow/deny/defer per the policy.
  - Loop-bound tests: `max_tool_iterations` caps the loop; repeated-batch and
    failed-command guards trip deterministically.
  - `update_plan` integration: a model `update_plan` tool call surfaces in the
    REVIEW phase and `/approve` gates mutating tools.
  - `run_command` secret-canary: scrubbed env contains no `ZAI_API_KEY`-style
    values.

## Evidence

- Oracle tool registry: `glm_acp/tools.py:205-404` (core tool schemas).
- Oracle agent loop: `glm_acp/agent.py:2837` (`run_loop`), inner dispatch
  `agent.py:3240-3460` (stream → `execute_tool` → `role: tool` feed-back).
- `DESTRUCTIVE_TOOLS` + mode eligibility: `glm_acp/config.py:639`,
  `agent.py:2843-2872`.
- `update_plan` flow: `agent.py:3537`, `4618` (`_handle_update_plan`),
  `4748` (`_send_plan`).
- Rust tool primitives: `crates/vesper-domain/src/tool.rs:124` (`ToolDefinition`
  + `ToolExecutionClass`), `:144` (`ToolCall`).
- Runtime single-turn contract (preserved): `crates/vesper-runtime/AGENTS.md`;
  tool-call termination `supervisor.rs:1262`.
