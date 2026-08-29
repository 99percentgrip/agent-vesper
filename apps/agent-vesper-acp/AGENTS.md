# Agent Vesper ACP executable

## Purpose

Compose configuration, the GLM factory, minimal runtime, ACP adapter, stdio
transport, stderr-only tracing, and orderly shutdown.

## Local Contracts

- Contain no session, provider-wire, or ACP-mapping business logic.
- Stdout is exclusively newline-delimited ACP JSON-RPC.
- Tests use loopback endpoints and synthetic credentials only.
- No provider child process or raw credential I/O is created by the ACP
  composition itself. The shared `vesper-harness` service may perform bounded
  workspace-scoped tool I/O and MCP/plugin subprocess work only after a model
  tool call passes the agent permission gate. The explicit `--setup`
  authentication command is the sole credential write and delegates the
  atomic private-file operation to the GLM provider auth boundary.
- Session readers are disabled unless explicitly enabled, use bounded
  filesystem stores, reject unsafe roots, and never create missing roots.
- Session writers are disabled unless explicitly enabled via
  `AGENT_VESPER_ENABLE_SESSION_WRITES`; the application constructs and injects
  `VesperSessionWriter` but delegates every mutation, atomic rename, and
  sidecar generation to `vesper-sessions`. The write root defaults to the Agent
  Vesper read root (`AGENT_VESPER_SESSION_WRITE_ROOT` or
  `AGENT_VESPER_SESSION_ROOT`) and must be absolute with an existing parent;
  `AGENT_VESPER_SESSION_WRITE_MAX_BYTES` bounds the record size.
- Durable editor-chat registration must enable both session writes and Agent
  Vesper session reads; writes commit completed turns, while reads provide
  list/load/resume after the editor launches a fresh ACP process. Checkpoint
  enablement is independent and is not required for conversation persistence.
- Provider selection is a composition-boundary concern resolved before the
  runtime is constructed. Production registers BOTH real adapters (Z.ai GLM
  + LM Studio) in every boot so the ACP `provider` footer picker (TUI
  `/provider` parity) can switch between them mid-session. The initial
  acting provider comes from the `--provider` flag or
  `AGENT_VESPER_PROVIDER` (accepted tokens: `glm`/`zai`, `lmstudio`). The
  deterministic synthetic adapter is reachable only through the
  `integration-test-harness` feature and must never be advertised as a real
  provider or model. The runtime stays provider-neutral; provider-specific
  configuration, credential overrides, and endpoint identity apply only to
  the selected adapter.
- The default endpoint assigned to freshly created sessions is injected by the
  composition boundary so persisted records carry a stable endpoint identity:
  `zai-coding` for the GLM adapter, `lmstudio-local` for the LM Studio
  adapter, and `synthetic` for the synthetic adapter.
  The runtime stays provider-neutral.
- The non-default `integration-test-harness` feature may compose generic
  synchronization wrappers, but the default release binary must not contain a
  dispatch gate or scenario behavior.
- The default ACP composition injects `AcpHarnessEngine`, which owns bounded
  per-session conversation history and routes prompts through `AgentLoop`.
  Agent-loop failures are projected into bounded safe classifications (for
  example context limit, rate/quota category, interrupted stream, or loop
  detection) before crossing the ACP boundary; never collapse all failures
  into an unactionable generic harness error and never expose raw provider
  payloads.
  Interrupted outcomes are transactional prompt responses: partial assistant
  text and plan remain in history/persistence, while the bounded diagnostic
  states the cause and whether recovery was withheld because a tool call had
  started.
  The engine also owns the TUI-parity feature surface (see the parity
  contract below): the cognitive-memory bundle, VRO orchestration, the
  tool-enforcement and cognitive-capability system instructions, and the
  silent pre-reply memory recall injection.
  The composition also injects the multi-provider footer control surface
  (`src/controls.rs`): the adapter advertises `provider` (TUI `/provider`
  parity — lists every registered adapter with live credential status in
  each description; switching stamps `vesper:active-provider` into the
  session envelope and swaps the acting `QualifiedModelId` so the next turn
  dispatches to the selected adapter; GLM keeps its `zai:` overrides across
  round trips; unauthenticated GLM descriptions tell the user to run
  `--setup`) plus the controls of the **acting provider only** (PRD
  `docs/provider-capability-gating-prd.md`): when `zai` acts, the full
  oracle-parity GLM set — `model` (MoA picker first, then the plan's
  models), `thought_level` (deep levels only on deep-reasoning models),
  `api_endpoint`, `generation_profile`, `auxiliary_model`, `mixture_mode`;
  when `lmstudio` acts, ONLY a truthful `model` picker fed by the adapter's
  cached native `/api/v1/models` catalog (verified LM Studio schema: live
  model ids, advertised context sizes; the pinned settings model always
  present as the offline fallback; no GLM plans/thinking/generation/
  auxiliary/mixture controls — the OpenAI-compatible wire carries none of
  them). GLM-only selections made while another provider acts are rejected
  fail-closed (never a silent cross-provider route). `permission_mode` is
  always advertised. Selections are validated against that surface,
  dispatched to the runtime `UpdateProviderConfiguration` command, and
  applied to the engine's turn configuration (footer picks take effect on
  the next turn; engine slash-command overrides layer on top). The
  adapter's `context_window` follows the acting provider — GLM's frozen
  per-model sizes for `zai`, the LM Studio model's advertised
  `max_context_length` for `lmstudio` (conservative 8K floor when
  unadvertised; never GLM's 1M for a local model) — so the Zed token
  counter (`usage_update`) sizes against the selected model.
  `tests/provider_selection.rs` is `integration-test-harness`-gated: the
  synthetic boot token it uses exists only under that feature.
  The engine executes the 28-command oracle slash catalog in-process
  (ADR 0010 Tier C) with full TUI harness parity: catalog commands answer
  from the harness executor with no provider dispatch, `/max-iterations` and
  model/plan switches persist as per-session engine overrides, unknown `/`
  text answers with the oracle's bounded unknown-command fallback, and every
  host-owned command is really wired — `/checkpoint`, `/rollback`, `/undo`,
  `/export`, `/sessions`, `/lineage`, `/ci`, `/plugins`, and `/mcp` run on
  the shared `vesper-harness` host-command executor; the checkpoint-family
  commands are opt-in per the gating contract above (when enabled, they run
  against the same durable checkpoint/MCP roots the TUI uses, and
  `/checkpoint` and `/lineage` seed a session lineage record named for the
  ACP session id); `/compact`,
  `/clear-history`, and `/clear-plan` mutate the engine's own per-session
  history and plan maps (`/clear-plan` republishes an empty plan update);
  `/usage` queries the live provider quota endpoint (truthful
  no-integration error for providers without one); `/diff` and `/release`
  replace the prompt with the TUI's workflow text and drive one real agent
  turn. Slash turns report `persist_turn == false` and never enter
  conversation history or persisted records — except the `/diff` and
  `/release` workflow turns, which persist like ordinary prompts. The
  engine's progress port pairs tool started/finished events by the most
  recently issued id per tool name and records the latest per-session plan
  markdown. The adapter injects a live ACP `session/request_permission`
  port for mutating tools; rejection, cancellation, unavailable clients,
  and malformed outcomes remain fail-closed. The engine injects the shared
  hosted Python-oracle tool surface and bounded project instruction
  context, and opens `MemoryStores` with the cross-project global skill
  layer (`AGENT_VESPER_GLOBAL_MEMORY_ROOT` → `~/.agent-vesper/memory`), so
  `/skills` lists global learned skills exactly like the TUI. Set
  `AGENT_VESPER_FULL_HARNESS=0` only for protocol-conformance fixtures that
  must exercise the provider-neutral single-turn runtime path; production
  defaults to the full engine.

## TUI↔ACP Parity Contract

Every host-agnostic capability shipped in the TUI MUST also be wired here
(root `AGENTS.md` Project Contracts). Current ledger:

- **Cognitive memory** (`src/cognition.rs`, ACP composition of the shared engine):
  `CognitionBundle::open_default` opens the SAME durable stores as the TUI
  (project `.agent-vesper/cognition/` + global
  `~/.local/share/agent-vesper/cognition/`), honors the same
  `embedding.json` (ADR 0016) source selection (local / lmstudio /
  bigmodel / provider-routed), routes extraction Zai → LM Studio → NoOp,
  and re-embeds on embedder-model change. Surface: silent pre-reply recall
  injection (restored out of history before persist, TUI parity), the
  `/remember` `/recall` `/forget` `/memories` `/promote` `/demote`
  `/embedding` slash family (including live `/embedding set` and
  `/embedding clear`), the shared `cognitive_capability_instruction`, and
  the VRO-7 procedural-memory learning sink. Changes to this module MUST
  be evaluated in the TUI composition (and vice versa). The model-facing
  instruction and 12-command host-parity catalog are shared foundation
  constants with registration/advertisement tests.
- **VRO reasoning orchestration**: opt-in via `AGENT_VESPER_VRO_ENABLED=1`
  (TUI parity). `should_orchestrate` routes non-Direct, non-ReAct profiles
  through `VroOrchestrator::execute_with_learning` with an
  `AcpCandidateGenerator` bridging the shared `AgentLoop`; `/reasoning set
  mode=<auto|fast|balanced|deep|maximum|off>` overrides the profiler
  per-session; strategy and ✓ LEARNED notices surface as
  `ReasoningDelta` events (the client's reasoning channel).
- **Tool-enforcement instruction**: `tool_enforcement_instruction()` (TUI
  VRO-11.5 text minus the `request_human_review`/`request_human_input`
  lines — this host does not register those tools).
- **Justified exclusions** (host-specific UX): TUI rendering niceties
  (single-column layout, markdown renderer, scrollbar, bracketed paste,
  F-keys), push-to-talk voice (interactive terminal capture),
  VesperLens browser interview + `request_human_review`/`request_human_input`
  tools (browser-only UX; a future ACP mapping needs an owner design
  decision), `/interview-limit` (VesperLens-scoped), and terminal-only
  catalog commands. ACP advertises the frozen 28-command compatibility
  catalog plus the shared implemented host-neutral extension catalog.

## Checkpoints and Lineage Are Opt-In

`/checkpoint`, `/rollback`, `/undo`, `/sessions`, and `/lineage` are
DISABLED by default in this composition (root contract): the shared
`HarnessToolService` is built with `new_with_checkpoint_gate(..., false)`
unless `AGENT_VESPER_ENABLE_CHECKPOINTS` is truthy or
`AGENT_VESPER_CHECKPOINT_ROOT` is set explicitly. Gated commands answer
with the opt-in notice and the service creates no durable
checkpoint/lineage directories at boot. `/ci` stays available (read-only);
`/export` writes an explicit user-requested file only. The TUI host keeps
its always-on default because it is user-launched interactively.

## Mid-Turn Slash Grace (CANCEL_GRACE)

Editors interrupt a running turn by sending `session/cancel` immediately
followed by the new prompt — even when that "prompt" is an informational
slash command. The adapter holds engine-session cancels for 400ms
(`CANCEL_GRACE` in `crates/vesper-acp/src/adapter.rs`): a prompt from
`CONCURRENT_SAFE_SLASH_COMMANDS` (`/status`, `/usage`, `/max-iterations`,
`/memory`, `/skills`, `/reasoning`, the cognition family, … — read-only
reports and next-turn overrides whose stores are independent of the live
turn) arrives inside the window and ABORTS the cancel, so the turn keeps
working while the slash answers concurrently and its text lands in the
session context. Mutating/turn-driving commands (`/compact`,
`/clear-history`, `/checkpoint`, `/diff`, `/release`, `/plugins`, `/mcp`,
…), any non-slash prompt, and grace expiry still perform the cancel.
`tokio::select!` is `biased` with notifications polled first so the
cancel+prompt pair is always evaluated together. The engine tracks
in-flight cancellations as a per-session SET (`Arc::ptr_eq` removal,
cancel-all on `session/cancel`) — concurrent turns on one session must
never overwrite each other's cancellation entry. Regression suite:
`apps/agent-vesper-acp/tests/midturn_slash_grace.rs` (real binary, slow
loopback provider).

## Verification

- Run process transcript tests with isolated environment roots.
- Run the full-harness ordered-stream regression; it must preserve reasoning
  and content delta order, emit final content exactly once, and accept every
  update at the physical writer before `end_turn`.
- Run `process_blockers` with `--all-features`; the guarded test driver is
  unavailable otherwise.
- Verify stdout purity and stderr secret-canary absence.
- Run `cargo test -p agent-vesper-acp --lib --bins` for read-configuration
  tests without invoking process transcript suites.
- Run `cargo test -p agent-vesper-acp --tests --all-features` for the complete
  real-process suite; every persistence vector must prove exact hash, file-set,
  length, and modification-time invariance.

## Child DOX Index

No children.
