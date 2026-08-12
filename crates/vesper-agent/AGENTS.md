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
  helper. `ToolResult` carries a `text` field plus an `injected_tools` channel
  (deferred-loading Phase 2): a tool may return additional `ToolDefinition`s
  that the agent loop splices into its advertised pool for the next
  iteration. `ToolResult` derives only `PartialEq` (not `Eq`) because
  `Vec<ToolDefinition>` is not `Eq`-derivable.
- `src/confinement.rs` — path-confinement *enforcement* (`confine`). `vesper-security`
  ships only authority descriptors, so the executor layer owns the
  canonicalize/boundary enforcement here (symlinks followed; escapes fail closed).
- `src/tools.rs` — the nine core parity-critical **real** executors (`read_file`,
  `write_file`, `edit_file`, `apply_patch`, `list_directory`, `search_files`,
  `grep`, `run_command`, `update_plan`) with confined filesystem/shell I/O.
- `src/registry.rs` — `ToolRegistry`: name → executor routing + mode-filtered
  `definitions_for`. As of the deferred-loading Phase 1, `definitions_for`
  also excludes any `ToolDefinition` whose `defer_loading` is `true` — those
  tools remain registered for execution but are hidden from the initial
  advertisement so the model does not see them until they are surfaced on
  demand.
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
  clone a loop with per-turn provider/model configuration. As of
  deferred-loading Phase 2, the `advertised_tools` binding is mutable per
  turn: when an executor returns `ToolResult.injected_tools`, the loop
  merges them (deduplicated by `ToolId` or `harness_name`) into the
  advertised pool so the next iteration advertises them to the model.
- `src/vro/mod.rs` — Vesper Reasoning Orchestrator (VRO) scaffolding.
  `VroOrchestrator` holds a `ReasoningConfig` (from `vesper-domain`), a
  `TaskProfiler`, and a shared `VerifierRegistry` (behind an `Arc` so the
  orchestrator stays `Clone`). `route(user_message, mode)` →
  `VroRoutingDecision` profiles the request and returns **`Orchestrate`** for
  non-`Direct` strategies when the flag is on (VRO-2.3), or `Direct` when
  disabled / in `Off` mode / the profile is `Direct` — the host then uses the
  unchanged `agent_loop.rs` direct path. `execute(request, generator,
  workspace_root)` runs the strategy loop via a caller-supplied
  [`CandidateGenerator`](crate::vro::orchestrator::CandidateGenerator) (the
  provider seam — the orchestrator never makes a provider call itself).
  `execute_with_judge(request, generator, workspace_root, judge, seed)` is the
  VRO-4 extension that supplies an optional `CandidateJudge` plus a
  deterministic shuffle `seed` for `ParallelCandidatesJudge`. Dispatch:
  `GenerateVerifyRepair` → `run_generate_verify_repair` (VRO-2.3),
  `ParallelCandidatesConsensus` → `run_parallel_candidates_consensus` (VRO-4),
  `ParallelCandidatesJudge` → `run_parallel_candidates_judge` (VRO-4) or
  degrades to consensus when no judge is supplied.
  `execute_react(request, agent, invoker, workspace_root)` is the VRO-5.1
  entry point for `ToolGroundedReact` (PRD §11.6); it is the only public
  method that supplies the `ReactAgent` + `ToolInvoker` seams. Other non-`Direct`
  strategies fall back to a single generate-and-verify pass until their
  dedicated executors land. **VRO-5.1 dispatch guard:** `execute` and
  `execute_with_judge` deliberately return `Failed` (with a clear "use
  `execute_react`" risk message) when the profiled strategy is
  `ToolGroundedReact`, so callers cannot silently run a tool-grounded prompt
  through the GenerateVerifyRepair baseline. This module performs no I/O,
  holds no provider handles, and never touches `AgentLoopConfig`,
  `AgentLoop`, the tool registry, or the permission gate.
- `src/vro/react.rs` — VRO-5.1 Tool-Grounded ReAct loop (PRD §11.6).
  `ReactAgent` trait (async object-safe via boxed `Send` future — single
  branch, no `boxed_clone` needed) is the provider seam: `next_action(prompt,
  trajectory)` returns either `CallTool { name, arguments }` or `Finish {
  output }`. `TrajectoryEntry` (Action | Observation) is the append-only
  per-turn transcript the agent consults. `ToolInvoker` trait (async
  object-safe) is the executor + permission seam: `class_of(name)` returns
  the `ToolExecutionClass` for Read-Before-Write, and `invoke(name, args)`
  routes through the existing permission gate and executor.
  `RegistryToolInvoker` is the production impl — wraps the same `ToolRegistry`
  + `check_tool_permission` + `PermissionPort` as
  `AgentLoop::gate_and_execute`, so operating mode and one-time approval are
  honored identically to the direct path. `run_tool_grounded_react(prompt,
  agent, invoker, budget, requires_grounding)` drives the loop: THINK (ask
  agent for next action) → ACT (route through invoker) → OBSERVE (append
  result text or structured failure). Halts on `Finish` (Succeeded),
  `max_model_calls` exhausted (BudgetExceeded), or `max_tool_calls` exhausted
  when the agent still wants tools (BudgetExceeded). **Read-Before-Write
  policy:** when `requires_grounding == true` and the agent attempts a
  mutating tool before any `ReadOnly` observation exists, the loop synthesizes
  a rejection observation and continues — the rejected attempt does NOT
  consume a `max_tool_calls` unit (it never reached the executor). **Tool
  errors become observations:** `ToolInvocationError` variants (UnknownTool,
  InvalidArguments, PermissionDenied, ExecutionFailed) are converted to
  structured failure text and fed back to the model so the loop can
  self-correct. Zero-breakage: only invoked via `execute_react`; `Direct`,
  `GenerateVerifyRepair`, and parallel paths never reach this code.
- `src/vro/orchestrator.rs` — VRO-2.3 Generate-Verify-Repair loop (PRD §11.3,
  §10.9). `CandidateGenerator` trait (async object-safe via boxed `Send`
  future; **`boxed_clone` is required** so VRO-4's parallel executor can give
  each `tokio::task::spawn` branch an owned `'static` generator handle),
  `GeneratedCandidate`, and `run_generate_verify_repair(...)`: generate
  → verify all mandatory verifiers → halt on all-pass (`Succeeded`) / any
  `VerificationStatus::Error` (`Inconclusive`) / `max_repairs` exhausted
  (`Failed`) / `max_model_calls` safety bound (`BudgetExceeded`) / non-repairable
  failure (`Failed`); otherwise consume one repair unit, feed the failed
  verifiers' findings back to the generator as corrections, and re-generate.
  Tested with fakes (no real provider / no real cargo) for deterministic
  halt-condition coverage.
- `src/vro/executor.rs` — VRO-4 Parallel Candidate Executor (PRD §10.6 +
  §11.4/§11.5). `CandidateExecutor::fan_out(generator, prompt, requested,
  budget)` spawns N concurrent `tokio::task::spawn` branches, each receiving a
  deeply-cloned isolated `BranchContext` (mutations in one branch never leak
  into siblings), assigns deterministic monotonic `CandidateId`s
  (`cand-0000`, `cand-0001`, …), and returns the aggregated outcomes sorted by
  ID. The requested branch count is **capped** at `budget.max_parallel_branches`
  (zero-cap errors clearly). `XorShiftRng` is a tiny deterministic seedable
  PRNG (xorshift32) used by the Judge strategy to shuffle candidates without
  adding a `rand` dependency on `vesper-agent`. Zero-breakage: this module is
  only invoked by the parallel-strategy handlers; `Direct` and
  `GenerateVerifyRepair` paths never reach it.
- `src/vro/strategies.rs` — VRO-4 strategy handlers (PRD §11.4 + §11.5).
  `normalize_output` strips whitespace + sorts JSON keys for canonical
  comparison (PRD §11.4: "compare normalized final answers and supporting
  evidence, not just wording similarity"). `quorum_threshold(n) = n.div_ceil(2)`.
  `run_parallel_candidates_consensus(...)` fans out → consensus_winner → on
  quorum `Succeeded`, else `Inconclusive` with the disagreement surfaced as
  an unresolved risk (PRD §18). `CandidateJudge` trait (async object-safe) is
  the model-based judge seam. `run_parallel_candidates_judge(...)` fans out →
  **shuffles** the candidates via `XorShiftRng` (PRD §11.5: "candidates in
  randomized order to reduce position bias") → asks the judge for a pick in
  the SHUFFLED view → maps the shuffled index back to the original
  `CandidateId` → returns `Succeeded`. The seed is exposed so tests can
  reproduce an exact shuffle.
- `src/vro/profiler.rs` — VRO-2.1 deterministic `TaskProfiler`: converts a
  user prompt (or `ReasoningRequest`) into a `TaskProfile` using pure
  keyword/substring heuristics (**no LLM call**, no `regex` dependency — the
  workspace `regex` lacks `unicode-perl`, so `\b`/`\w` reject; keyword
  detection uses case-insensitive `str::contains`). Pipeline: chat bypass
  (short + no code + no action verb + **no grounding signal** → `chat`/`Direct`;
  VRO-5.1 added the grounding-signal guard so prompts like "what does the
  main.rs file do?" no longer bypass to Direct) → domain mapping (mutation
  verbs > math > planning > research > code indicators > chat) → risk
  (`delete`/`commit`/`production` → `High`) → grounding + verifiers
  (`.rs` → `cargo_check`/`cargo_test`/`clippy`) → complexity/ambiguity →
  §12 strategy ladder (**VRO-5.1:** the Low/Low → Direct shortcut now yields
  when `requires_grounding == true`, so grounded non-mutation prompts route
  to `ToolGroundedReact`). `profile_request` honors a caller `risk_hint`
  override.
- `src/vro/verifiers.rs` — VRO-2.2 deterministic verifier registry (PRD §10.8).
  Async object-safe `Verifier` trait (boxed `Send` future — the workspace has
  no `async_trait`/`trait-variant` dep, so the trait returns
  `Pin<Box<dyn Future + Send>>` directly), `VerificationContext`
  (workspace root + evidence refs), and `VerifierRegistry` keyed by
  `verifier_id` (`register`/`contains`/`ids`/`run`; `default_cargo()` preloads
  `cargo_check` + `cargo_test`). `CargoCheckVerifier` runs
  `cargo check --message-format=json` and parses compiler diagnostics into
  `VerificationFinding`s (pure `parse_findings` is unit-testable without
  cargo); `CargoTestVerifier` runs `cargo test` and maps failures. Both shell
  out via `std::process::Command` offloaded to `tokio::task::spawn_blocking`.
  Compiler/test failures are `repairable: true`; a verifier that cannot run
  (cargo missing, crash) returns `VerificationStatus::Error` (distinct from
  `Failed`). No new dependencies.
- `src/providers/mod.rs` — VRO-3.1 provider adapters that implement the
  `vesper-agent`-owned [`CandidateGenerator`](crate::vro::CandidateGenerator)
  seam. Lives in `vesper-agent` (not a `vesper-provider-*` crate) because the
  generation seam is a `vesper-agent` trait; a provider implementing it would
  otherwise invert the crate dependency direction.
- `src/providers/lmstudio/` — VRO-3.1 LM Studio local/LAN model-server adapter
  (PRD §13). `config.rs` (`LmStudioConfig`: `api_base_url` + an opaque
  `LmStudioApiKey` newtype + optional model; the key is `#[serde(skip)]` and
  wrapped to satisfy the secret-shape xtask guard); `client.rs` (pure HTTP
  request builders — `build_models_request` / `build_chat_request` — plus the
  async `LmStudioTransport` trait port, mockable in tests; NO HTTP client crate
  imported — the real transport is the composition-boundary concern);
  `discovery.rs` (`/models` discovery + `probe_capabilities` +
  `CapabilityRegistry`); `generator.rs` (`LmStudioCandidateGenerator`
  implementing `CandidateGenerator`, mapping `(prompt, corrections)` → chat
  messages with the failed verifiers' findings fed back as a corrections
  message, bearer-auth injected). No live LM Studio integration in VRO-3.1 (per
  execution constraints).

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
- The advertised tool pool starts as `definitions_for(mode)` (which itself
  excludes `defer_loading == true` tools) and is **mutable** across turns.
  When an executor returns `ToolResult.injected_tools`, the loop merges those
  schemas into the advertised pool, deduplicating by `ToolId` or
  `harness_name`, so the next iteration advertises them. This is the
  Claude Code-style deferred-loading seam — the loop never re-references
  the registry between turns, so injected schemas live only inside the
  per-turn advertised list (Phase 2 does not register them for execution;
  that is a future phase's concern).
- **Phase 3 gateway routing.** `ToolRegistry` carries an optional list of
  `(prefix, executor)` gateways registered via `with_gateway`. When a tool
  name is not in `entries` but matches a registered prefix, `execute()`
  routes to the longest-matching gateway executor. `gate_and_execute`
  looks up definitions from the loop's live advertised pool (covering
  injected schemas) and falls back to `ToolRegistry::definition()` (so a
  hallucinated call to a registered-but-mode-filtered tool like
  `write_file` in Plan mode is still denied by the permission gate rather
  than reported as "unknown tool"). The composition boundary wires the
  `McpGatewayExecutor` under the `mcp__` prefix so dynamically discovered
  MCP tools can be executed after they are injected and advertised.
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
  test for the composition boundary. To opt a tool into Claude Code-style
  deferred loading (hide it from the initial advertisement while keeping it
  executable), set `ToolDefinition.defer_loading = true` on the registered
  definition; `definitions_for(mode)` will then exclude it from both `Plan`
  and `Code` mode advertisement, but `contains`/`definition`/`execute` keep
  working by name.

## Verification

- Run `cargo test -p vesper-agent`.
- Run `cargo xtask verify` (fmt + clippy + workspace tests + architecture).
- `cargo run --package xtask --quiet -- architecture` must include
  `vesper-agent` and validate its dependency direction.

## Child DOX Index

No children.
