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
  method that supplies the `ReactAgent` + `ToolInvoker` seams.
  `execute_with_critic_adjudicator(request, generator, workspace_root, judge,
  critic, adjudicator, seed, criteria)` is the VRO-6 entry point for
  `ProposerCriticAdjudicator` (PRD §11.8); it is the only public method that
  supplies the `CandidateCritic` + `Adjudicator` seams. Dispatch:
  `GenerateVerifyRepair` → `run_generate_verify_repair` (VRO-2.3),
  `ParallelCandidatesConsensus` → `run_parallel_candidates_consensus` (VRO-4),
  `ParallelCandidatesJudge` → `run_parallel_candidates_judge` (VRO-4) or
  degrades to consensus when no judge is supplied,
  `BoundedTreeSearch` → `run_bounded_tree_search` (VRO-6),
  `ProposerCriticAdjudicator` → `run_proposer_critic_adjudicator` (VRO-6) or
  degrades to consensus when no critic + adjudicator is supplied (via
  `execute_with_judge`) / when critic or adjudicator is `None` (via
  `execute_with_critic_adjudicator`). **VRO-7 entry point:**
  `execute_with_learning(request, generator, workspace_root, judge, critic,
  adjudicator, agent, invoker, seed, criteria, sink, extractor,
  extracted_at)` is the single composition-boundary method that fans in
  every optional strategy seam AND layers Verified Workflow Learning
  (PRD §11.9) on top. It dispatches to `run_tool_grounded_react_with_trajectory`
  for `ToolGroundedReact`, to `execute_with_critic_adjudicator` for PCA, or
  to `execute_with_judge` for everything else; on `Succeeded` AND a
  learning-eligible strategy it runs the `WorkflowExtractor` and persists
  the resulting `ProceduralMemory` through the optional
  `ProceduralMemorySink`. **Zero-breakage guarantee:** extraction errors
  surface as one extra `unresolved_risks` entry ("workflow-learning
  skipped: …"); sink errors surface as "workflow-learning persistence
  skipped: …"; `sink == None` still records the extraction ("workflow-
  learning extracted (no sink): …"). The orchestrator never panics from a
  learning error and never modifies the underlying turn outcome.
  **VRO-5.1 dispatch guard:** `execute` and
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
  agent, invoker, budget, requires_grounding)` is the canonical entry point;
  `run_tool_grounded_react_with_trajectory(...)` (added VRO-7) is the
  sibling that ALSO returns the accumulated `TrajectoryEntry` sequence on
  every terminal path (Succeeded AND BudgetExceeded), so the
  `WorkflowExtractor` can summarize whatever progress was made. The
  canonical `run_tool_grounded_react` is a thin wrapper that discards the
  trajectory for callers that do not need VRO-7 learning. Both functions
  drive the loop: THINK (ask agent for next action) → ACT (route through
  invoker) → OBSERVE (append result text or structured failure). Halts on
  `Finish` (Succeeded), `max_model_calls` exhausted (BudgetExceeded), or
  `max_tool_calls` exhausted when the agent still wants tools
  (BudgetExceeded). **Read-Before-Write policy:** when
  `requires_grounding == true` and the agent attempts a mutating tool before
  any `ReadOnly` observation exists, the loop synthesizes a rejection
  observation and continues — the rejected attempt does NOT consume a
  `max_tool_calls` unit (it never reached the executor). **Tool errors
  become observations:** `ToolInvocationError` variants (UnknownTool,
  InvalidArguments, PermissionDenied, ExecutionFailed) are converted to
  structured failure text and fed back to the model so the loop can
  self-correct. Zero-breakage: only invoked via `execute_react` /
  `execute_with_learning`; `Direct`, `GenerateVerifyRepair`, and parallel
  paths never reach this code.
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
  **VRO-9 strict budget enforcement (PRD §10.4 "Budget Manager"):** the loop
  captures `started_at: Instant` at entry and checks all THREE ceilings —
  (a) `max_wall_time_ms` is checked BEFORE every Generate (a tight ceiling
  fires before any model call), (b) `max_total_output_tokens` is checked
  AFTER every Generate against the cumulative `cost.total_tokens` (catches
  a runaway repair loop), (c) `max_model_calls` is checked after the
  verify pass. Each breach returns `OutcomeStatus::BudgetExceeded` with the
  breached-ceiling name in the `unresolved_risks` note (PRD §10.4: "Emit
  budget-exhaustion reasons"). Tested with fakes (no real provider / no
  real cargo) for deterministic halt-condition coverage including the new
  token-budget and wall-clock test cases.
- `src/vro/executor.rs` — VRO-4 Parallel Candidate Executor (PRD §10.6 +
  §11.4/§11.5). `CandidateExecutor::fan_out(generator, prompt, requested,
  budget)` spawns N concurrent `tokio::task::spawn` branches, each receiving a
  deeply-cloned isolated `BranchContext` (mutations in one branch never leak
  into siblings), assigns deterministic monotonic `CandidateId`s
  (`cand-0000`, `cand-0001`, …), and returns the aggregated outcomes sorted by
  ID. The requested branch count is **capped** at `budget.max_parallel_branches`
  (zero-cap errors clearly). `XorShiftRng` is a tiny deterministic seedable
  PRNG (xorshift32) used by the Judge strategy to shuffle candidates without
  adding a `rand` dependency on `vesper-agent`. **VRO-9 race-aware fan-out
  (PRD §10.6 "Branch cancellation" + "Early stopping"):**
  `fan_out_with_early_stop(generator, prompt, requested, budget, early_stop)`
  is the opt-in extension that races branches with `tokio::select!` over
  their `JoinHandle`s and calls `early_stop(&outcome)` on each completion.
  When the predicate fires, the executor `JoinHandle::abort`s every
  still-pending sibling and returns the partial outcome set (PRD §10.4:
  "Respect cancellation immediately"; §10.4: "Stop low-value branches").
  The plain `fan_out` delegates to this with `|_| false` (zero-behavior-
  change backward-compat). The aborted siblings never reach their post-yield
  completion counter — verified by the
  `early_stop_aborts_pending_sibling_branches` test. **VRO-9 cross-model
  racing (PRD §10.6 "Cross-model candidates"):**
  `MultiModelCandidateGenerator::new(Vec<Box<dyn CandidateGenerator>>)`
  is a generator wrapper that round-robins each call across the configured
  provider pool (`provider_for_index(n) = providers[n % len]`). It exposes
  `generate` + `boxed_clone` like every `CandidateGenerator` and shares an
  `Arc<AtomicUsize>` call counter across clones so spawned branches route
  deterministically under VRO-4 parallel fan-out. Rejects an empty pool
  with `MultiModelError::EmptyProviderPool`. Zero-breakage: only invoked
  by the parallel-strategy handlers; `Direct` and `GenerateVerifyRepair`
  paths never reach it. **VRO-10 candidate-specific branch prompts (PRD
  §10.6 "Candidate-specific prompts"):** the new `BranchDiversification`
  enum (`None` | `SystemPromptVariants(Vec<String>)`) is applied by
  `fan_out_diverse(generator, prompt, requested, budget, diversification,
  early_stop)`. Each branch receives `diversification.prompt_prefix_for(i)`
  prepended to its prompt (the canonical `diverse_branches()` constructor
  ships the four-variant conservative → balanced → creative → highly
  creative stance ladder the directive names). `BranchDiversification::None`
  preserves byte-identical VRO-4 behavior; the existing `fan_out` /
  `fan_out_with_early_stop` are unchanged (zero-breakage). PRD §10.6:
  "Candidate diversity must not be simulated merely by asking for 'three
  alternatives' in one completion."
- `src/vro/repair.rs` — **VRO-10 Repair Controller heuristics (PRD §10.9).**
  Pure classification + hint surface: `classify_finding(&VerificationFinding)
  -> RepairHeuristic` matches against the finding's `message`, `severity`,
  and `location` to classify it as `JsonParse` / `SchemaMismatch` /
  `FileNotFound` / `CompilationError` / `TestFailure` / `ConstraintViolation`
  / `Generic`. Each non-Generic class carries a class-specific correction
  hint (`RepairHeuristic::correction_hint()`) the orchestrator injects into
  the next Generate's corrections vector. `RepairController::new()` holds
  the previous repair attempt's finding-message signature so
  `is_repeated_attempt(&[VerificationFinding])` can detect an identical
  retry (PRD §10.9: "Avoid repeating an identical failed attempt") — the
  orchestrator escalates to `Failed` rather than re-issuing the same
  prompt. Stateless, allocation-only, no I/O. Zero-breakage: the GVR loop
  consults the controller only when a repair is about to happen; a `Generic`
  finding injects no hint, preserving VRO-9 behavior for unclassifiable
  failures.
- `src/vro/rate_limit.rs` — **VRO-10 provider rate-limit accounting (PRD
  §10.4).** `RateLimitTracker` is a thread-safe atomic-backed accounting
  struct (`Arc<RateLimitTracker>` is shared between the provider adapter
  and the orchestrator). `record_429(retry_after_ms: Option<u64>)` is
  called by the provider adapter on HTTP 429; `status()` returns
  `Available` or `Blocked { retry_after_ms }` (auto-clearing past the
  deadline). The default `untracked()` tracker never blocks, so the GVR
  loop's behavior is byte-identical to VRO-9 when no tracker is wired.
  The GVR loop's pre-Generate check halts with
  `OutcomeStatus::RateLimitExceeded` when the tracker reports `Blocked`
  (PRD §10.4: "account for provider rate limits"). PRD §10.9
  "Avoid repeating an identical failed attempt" is enforced via the
  controller's signature comparison; rate-limit halts are NOT a repair —
  they are a hard stop until the operator clears the tracker.
- `tests/live_react_integration.rs` — **VRO-9 Directive 3** live HTTP
  integration tests for the Tool-Grounded ReAct loop (PRD §22.2 "Real LM
  Studio process"). Every test is `#[ignore]`-marked (skipped by default
  in standard CI) and the `endpoint_reachable()` skip-helper early-returns
  a clear message when LM Studio is offline at `localhost:1234`. The
  `LiveLmStudioReactAgent` impl is a self-contained SSE client backed by
  `reqwest` (declared as a **dev-dependency only** — `src/` never references
  `reqwest`, so the architecture scan passes); it mirrors the TUI's
  production LM Studio provider's OpenAI-compatible `data: <json>` /
  `[DONE]` parsing. Run locally with `cargo test -p vesper-agent --test
  live_react_integration -- --ignored`. Zero-breakage: the binary is
  never built by the canonical `cargo xtask verify` gate's
  `cargo test --workspace --all-features` unless explicitly invoked.
- `tests/soak_test.rs` — **VRO-10 Directive §22.4** soak tests (PRD §22.4
  "Long sessions / Repeated Deep-mode requests / Memory growth / Parallel
  sessions"). Five `#[ignore]`-marked tests loop the orchestrator through
  50+ back-to-back synthetic requests (in-process fakes, not network) to
  prove memory safety, thread-leak prevention, repair-controller signature
  boundedness, rate-limit-tracker atomic-counter integrity, and
  cross-turn state non-corruption. Standard `cargo test` skips them;
  developers run them with `cargo test -p vesper-agent --test soak_test
  -- --ignored --nocapture`. Zero-breakage: the binary is never built by
  the canonical `cargo xtask verify` gate's
  `cargo test --workspace --all-features` unless explicitly invoked.
- `src/vro/strategies.rs` — VRO-4 + VRO-6 strategy handlers (PRD §11.4 +
  §11.5 + §11.7 + §11.8). `normalize_output` strips whitespace + sorts JSON
  keys for canonical comparison (PRD §11.4). `quorum_threshold(n) =
  n.div_ceil(2)`. `run_parallel_candidates_consensus(...)` (VRO-4) fans out
  → consensus_winner → on quorum `Succeeded`, else `Inconclusive`.
  `CandidateJudge` trait (async object-safe) is the model-based judge seam;
  `run_parallel_candidates_judge(...)` (VRO-4) fans out → **shuffles** via
  `XorShiftRng` → asks the judge for a shuffled-index pick → maps back to the
  original `CandidateId`. `run_bounded_tree_search(...)` (VRO-6, PRD §11.7)
  expands a level-by-level tree of partial candidates up to
  `budget.max_search_depth`, fanning out `budget.max_parallel_branches`
  children per node. Each node is verified against the profile's mandatory
  verifiers: a **passing** node is a candidate best leaf (early-stop the
  entire search — PRD §10.6); a **Failed** verifier result (ran and found
  problems) is **pruned** (PRD §11.7 "aggressive pruning" + directive
  "abandoning a branch if a deterministic verifier fails early"); an
  **Error** result (verifier could not run — cargo missing, crash, or
  unregistered verifier like `clippy`) does NOT prune (PRD §10.8: Error is
  distinct from Failed — the candidate might be fine, we just couldn't
  check it); a **non-definitive** outcome (Error/Inconclusive/Skipped) at
  depth < max_depth is **expanded further** (refined prompt carries the
  parent's output forward). The total candidate
  count is bounded by `budget.max_model_calls` (PRD §22.3: no infinite
  search loop). `CandidateCritic` + `Adjudicator` traits (async
  object-safe) are the VRO-6 model-based seams;
  `run_proposer_critic_adjudicator(...)` (VRO-6, PRD §11.8) enforces strict
  role separation: **propose** (fan out via VRO-4 executor) → **critique**
  (per-candidate objective critique from `CandidateCritic`, anchored to
  explicit criteria) → **adjudicate** (`Adjudicator` selects from the
  (candidate, critique, criteria) triple, NOT from persuasive prose — PRD
  §11.8: "The adjudicator must evaluate explicit criteria, not select the
  most persuasive prose"). Zero-breakage: only invoked when the profiled
  strategy is `BoundedTreeSearch` or `ProposerCriticAdjudicator`.
- `src/vro/profiler.rs` — VRO-2.1 deterministic `TaskProfiler`: converts a
  user prompt (or `ReasoningRequest`) into a `TaskProfile` using pure
  keyword/substring heuristics (**no LLM call**, no `regex` dependency — the
  workspace `regex` lacks `unicode-perl`, so `\b`/`\w` reject; keyword
  detection uses case-insensitive `str::contains`). Pipeline: chat bypass
  (short + no code + no action verb + **no grounding signal** → `chat`/`Direct`;
  VRO-5.1 added the grounding-signal guard so prompts like "what does the
  main.rs file do?" no longer bypass to Direct) → **VRO-6 advanced-strategy
  detection** (BoundedTreeSearch keywords: "root cause", "debug complex",
  "migration sequence", "competing hypotheses", "irreversible consequences",
  "constraint-heavy", "tree/beam search", etc. → `BoundedTreeSearch`;
  ProposerCriticAdjudicator keywords: "high-consequence", "weak verifiers",
  "adjudicate", "high-stakes architecture/design", etc. →
  `ProposerCriticAdjudicator` with forced `High` risk floor) → **VRO-4
  parallel-strategy detection** (trade-off/alternatives → Judge; verify-claim
  → Consensus) → domain mapping → risk → grounding + verifiers →
  complexity/ambiguity → §12 strategy ladder (**VRO-5.1:** the Low/Low →
  Direct shortcut now yields when `requires_grounding == true`).
  `profile_request` honors a caller `risk_hint` override.
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
- `src/vro/learning.rs` — VRO-7 Verified Workflow Learning (PRD §11.9).
  Pure extraction + sanitization logic — no `vesper-cognition` dependency,
  no SQLite, no network I/O. (The architecture rule that `vesper-agent`
  depends only on domain/provider/runtime means cognition is a peer crate;
  persistence is delegated to a trait port supplied at the composition
  boundary, mirroring the VRO-4/5.1/6 pattern of `CandidateJudge` /
  `ToolInvoker` / `CandidateCritic`.) Public types:
  - `SecretScrubber` — compiles a priority-ordered pattern set ONCE at
    construction and reuses it across calls. Detects (1) AWS access keys
    (`AKIA[0-9A-Z]{16}`), (2) JWTs (three base64url segments starting with
    `eyJ`), (3) bearer tokens (`[Bb]earer\s+<token>`), (4) generic
    credential assignments
    (`(api[_-]?key|apikey|token|secret|password|passwd|auth_token|access_key)\s*[=:]\s*['"]?<value>`),
    (5) AWS secret access keys (40-char base64 after an `aws_secret` hint),
    (6) IPv4 addresses, and (7) high-entropy 32+ char base64/url-safe
    strings (Shannon entropy > 4.0 bits/char). Each match becomes a
    deterministic `[REDACTED:<KIND>]` placeholder. The high-entropy pass
    runs LAST so the deterministic placeholders (which contain only `[`, `]`,
    `:`, letters, underscore) cannot themselves trip the entropy threshold.
    `scrub_json` recursively redacts string values in `serde_json::Value`
    trees. **Pitfall** (VRO-7): the workspace `regex` dep is
    `default-features = false` (no `unicode-perl`), so `\b`/`\s`/`\w`
    REJECT with NFA-build errors. This crate declares a per-crate override
    `regex = { version = "1", default-features = false, features = ["std",
    "unicode-perl"] }` so the scrubber patterns compile; the rest of the
    workspace stays ASCII-only via `regex.workspace = true`.
  - `ProceduralMemory` + `ProceduralStep` — the persisted artifact. Each
    step is a *generalized* observation (`Invoke tool \`read_file\` with
    sanitized arguments.`, plus a sanitized JSON argument excerpt and a
    bounded 240-char observation excerpt). The `id` is a deterministic
    SHA-256 over the normalized `(objective, strategy, steps)` triple so
    two trajectories that generalize to the same procedure produce the
    same id (cognitive-memory dedupe). Round-trips through `serde_json`.
  - `WorkflowExtractor` — two entry points:
    `extract_from_trajectory(request, outcome, trajectory, strategy,
    extracted_at)` (ReAct path — walks each `Action`/`Observation` pair) and
    `extract_from_outcome(request, outcome, strategy, extracted_at)` (non-
    ReAct path — synthesizes a `generate` step from `final_output`, plus a
    `verify` step when verifiers ran). Both reject non-`Succeeded` outcomes,
    empty objectives, and empty trajectories with `LearningError`.
  - `ProceduralMemorySink` — async object-safe persistence port
    (`save_procedure(&ProceduralMemory) -> Result<String, LearningError>`).
    The composition boundary supplies the cognition-backed impl (which
    forwards to `vesper_cognition::pipeline::CognitiveMemory::add_procedural`
    behind the scenes). Tests use a `RecordingSink` fake.
  - `LearningError` — non-fatal error variants: `OutcomeNotSucceeded`,
    `PrivateRequestRejected` (PRD §17: PrivacyMode::Private requests must
    NOT be persisted; the extractor refuses BEFORE building the procedure so
    no scrubbed-but-still-private bytes can leak through a future sink bug),
    `NoStepsToExtract`, `EmptyObjective`, `SinkRejected`. The orchestrator
    converts every variant into one `unresolved_risks` entry; the turn
    itself never fails because of a learning error.
  - `is_learning_eligible(strategy)` — true for every complex strategy
    except `Direct` (no procedure to memorize for plain chat).
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
  message, bearer-auth injected). `react.rs` (VRO-5.2
  `LmStudioReactAgent` implementing the VRO-5.1 `ReactAgent` seam —
  `next_action(prompt, trajectory)` builds the ReAct prompting contract
  [`REACT_SYSTEM_PROMPT`] + user prompt + trajectory replayed as
  `assistant`/`user` message pairs, sends `/chat/completions` via the shared
  `LmStudioTransport`, and parses the response with the infallible
  `parse_react_decision`. The parser uses precedence: `action.tool` JSON →
  `CallTool`, `answer`/`final_answer`/`final` JSON → `Finish`, prose without
  JSON → `Finish` (graceful exit), JSON-shaped text that fails to parse or has
  an unrecognized shape → a synthesized `CallTool` with the sentinel
  `MALFORMED_TOOL_NAME` so the loop's `ToolInvoker` returns `UnknownTool` and
  feeds the failure back to the model as an observation for self-correction.
  The transport is `Arc<dyn LmStudioTransport>` (shared, cheap clone) so the
  agent mirrors `LmStudioCandidateGenerator`'s shape exactly). No live LM
  Studio integration in VRO-3.1 (per execution constraints).

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
  `regex` for search; `sha2` for VRO-7 deterministic procedure IDs; `tempfile`
  dev-only). Must NOT depend on `vesper-acp`, `vesper-sessions`, SQLite, MCP,
  frontends, or any disposable spike. **VRO-7 per-crate `regex` override:**
  `crates/vesper-agent/Cargo.toml` declares
  `regex = { version = "1", default-features = false, features = ["std",
  "unicode-perl"] }` (NOT `regex.workspace = true`) so the `SecretScrubber`'s
  `\b`/`\s`/`\w` patterns compile; this does NOT leak to other crates, which
  keep using the workspace's ASCII-only regex.
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
- **VRO-9 dev-only `reqwest` exception.** `crates/vesper-agent/Cargo.toml`
  declares `reqwest.workspace = true` under **`[dev-dependencies]` only** so
  the live ReAct integration test binary (`tests/live_react_integration.rs`)
  can talk to a real LM Studio endpoint. The production `src/` tree MUST NOT
  reference `reqwest` — `cargo xtask architecture`'s `scan_production_sources`
  scans `src/` only and would fail with "forbidden foundational reference
  `reqwest`" if the term appeared there. This is the same dev-only carve-out
  pattern TUI uses for `reqwest.workspace = true` in its production deps.

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
