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
  The composition also injects the multi-provider footer control surface
  (`src/controls.rs`, derived from the frozen GLM oracle catalog): the
  adapter advertises `provider` (TUI `/provider` parity — lists every
  registered adapter with live credential status in each description;
  switching stamps `vesper:active-provider` into the session envelope and
  swaps the acting `QualifiedModelId` so the next turn dispatches to the
  selected adapter; GLM keeps its `zai:` overrides across round trips;
  unauthenticated GLM descriptions tell the user to run `--setup`),
  `model` (MoA picker first, then the plan's models),
  `thought_level` (deep levels only on deep-reasoning models),
  `api_endpoint`, `generation_profile`, `auxiliary_model`, `mixture_mode`,
  and `permission_mode` as ACP `sessionConfigOptions` on
  `session/new`/`load`/`resume`/`set`, and `session/set_config_option`
  selections are validated against that surface, dispatched to the runtime
  `UpdateProviderConfiguration` command, and applied to the engine's turn
  configuration (footer picks take effect on the next turn; engine
  slash-command overrides layer on top). The adapter's `context_window`
  follows the frozen per-model context sizes so the Zed token counter
  (`usage_update`) sizes against the selected model.
  The engine executes the 28-command oracle slash catalog in-process
  (ADR 0010 Tier C) with full TUI harness parity: catalog commands answer
  from the harness executor with no provider dispatch, `/max-iterations` and
  model/plan switches persist as per-session engine overrides, unknown `/`
  text answers with the oracle's bounded unknown-command fallback, and every
  host-owned command is really wired — `/checkpoint`, `/rollback`, `/undo`,
  `/export`, `/sessions`, `/lineage`, `/ci`, `/plugins`, and `/mcp` run on
  the shared `vesper-harness` host-command executor against the same durable
  checkpoint/MCP roots the TUI uses (`/checkpoint` and `/lineage` seed a
  session lineage record named for the ACP session id); `/compact`,
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

## Verification

- Run process transcript tests with isolated environment roots.
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
