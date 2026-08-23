# ACP protocol adapter

## Purpose

Own official ACP Rust SDK integration, protocol-v1 compatibility, request and
update mapping, and bounded asynchronous dispatch into `vesper-runtime`.

## Local Contracts

- ACP SDK and wire types remain inside this crate.
- SDK callbacks enqueue bounded work and never await provider completion.
- The compatibility layer owns legacy `PromptResponse.userMessageId` placement.
- Stdout transport carries ACP JSON-RPC only.
- Session updates must reach the physical writer through bounded flow control;
  SDK-internal queues may not defeat runtime backpressure.
- Load/resume replay is visible history, plan, metadata/mode, available
  commands, then lifecycle response; every update is writer-accepted first.
- A successful prompt turn is persisted through the injected runtime writer
  before the prompt response is sent; the save runs inside the prompt's
  detached task so the dispatcher loop never blocks. A persistence failure
  surfaces a sanitized request error with a stable reason and the dispatcher
  continues serving other requests.
- The crate is provider-neutral, maps persistent read outcomes through runtime,
  and performs no direct persistence I/O.
- Corrupt, unsupported, bounded, denied, unsafe, workspace-mismatch, and
  write-failed records return sanitized request errors without terminating
  dispatch.
- Client-declared `mcpServers` on `session/new` are accepted and ignored
  (oracle parity): Zed attaches every configured MCP server to each session,
  so rejecting them made the agent unloadable with `-32602`. The harness MCP
  registry remains the sole MCP source; no client server is launched.
- Slash-command parity (ADR 0010 Tier C): `available_commands_update`
  advertises the 28-command `vesper-domain` catalog exactly once for
  `session/new`, `session/load`, and `session/resume`; `fork` advertises
  nothing (fixtures/acp/fork-session parity). On `session/new` the
  notification is sent only AFTER the response — clients such as Zed
  register the session when the response is processed and drop
  `session/update` notifications for unregistered sessions ("unknown
  session"), so a pre-response advertisement never reached the Zed
  slash-command menu. Load/resume sessions are already registered
  client-side, so those advertisements stay before the response (replay
  ordering). With an injected prompt engine, `/`-prefixed prompts route to
  the engine, which owns catalog execution; without an engine they fail
  closed with `-32601` instead of dispatching the provider. Slash turns
  report `AcpPromptResult.persist_turn == false` and are never appended to
  persisted sessions (the `acp.slash-command` fixture expects unchanged
  file hashes).
- `AcpEngineEvent`/`AcpEventSink` are the streaming-engine vocabulary
  (`ReasoningDelta`, `ContentDelta`, `ToolStarted`, `ToolFinished`, `Usage`,
  `PlanUpdated`); the adapter sink maps each to the same wire shape the
  single-turn event pump produces. Full-harness updates are serialized in
  emission order and drained through physical-writer acceptance before the
  terminal response. Live `PlanUpdated` markdown is converted to structured
  ACP `plan` entries (including status and priority) so editor clients render
  their native TODO surface; an empty plan publishes an empty entry list.
  A streamed content turn must not append the complete
  final text again; engines with no content deltas retain the one-chunk final
  fallback. Engines that lack tool-call ids pair
  started/finished calls by the most recently issued id per tool name (the
  agent loop executes tools strictly sequentially).
- Session config options: `thought_level` and `permission_mode` remain
  built-in runtime-modeled options (oracle option ids; the permission
  control advertises the oracle value `read`, which the setter accepts).
  Provider-owned footer controls (`model`, `api_endpoint`,
  `generation_profile`, `auxiliary_model`, `mixture_mode`) flow through
  `controls.rs`'s provider-neutral `SessionControlSurface` injected via
  `AcpAdapterConfig::controls`: the composition boundary contributes the
  descriptors (contribution order is the render order — model first),
  per-control live current-value resolvers read from the session envelope,
  and a provider-owned apply closure maps a validated `(id, value)` onto a
  merged envelope plus an optional new model. The adapter validates every
  selection against the contributed options (fail-closed
  `unsupported-session-config-value` for invented values), dispatches the
  runtime `UpdateProviderConfiguration` command, and re-advertises the
  options from the fresh snapshot on `session/new`/`load`/`resume`/`set`.
  Without an injected surface only the two built-in options exist.
- `AcpAdapterConfig::additional_commands` appends only composition-implemented
  host-neutral commands to the frozen 28-entry compatibility catalog. The
  default remains exactly 28 for oracle fixtures; production ACP appends the
  shared host-parity catalog and advertises it on new/load/resume.
- Session mode/config requests are mapped to runtime mode, reasoning, and
  permission updates; delete and logout have explicit protocol responses.
- `AcpPromptEngine` is an optional composition port. When injected, prompt
  requests route through a host's bounded multi-turn `vesper-agent` loop and
  are published with ACP backpressure. Cancellation notifications are routed
  to the injected engine as well. The adapter supplies a bounded
  `session/request_permission` bridge to injected engines; cancellation
  resolves any pending approval and the engine must fail closed on rejection
  or a missing bridge. Without an injected engine, the runtime single-turn
  path remains available for protocol conformance. Engine failures must be
  pre-sanitized, bounded, actionable classifications; the adapter preserves
  that safe reason in ACP error data instead of flattening every failure to a
  generic `harness engine failed` message.
- `vro_events.rs` (VRO-10, PRD §16) owns the VRO status-event vocabulary
  surfacing orchestrator phase transitions to upstream ACP clients. The
  `VroEvent` enum mirrors the 13 PRD §16 event names (`reasoning.profiled`,
  `reasoning.strategy_selected`, …, `reasoning.completed`); each variant
  carries `session_seq: u64` (PRD §16: "monotonic sequence numbers"),
  optional `branch_id` (PRD §16: "Parallel branch events must carry branch
  identifiers"), and a user-safe markdown `summary` (PRD §8.2).
  `translate_vro_event_to_acp(session_id, &VroEvent)` translates each event
  into an ACP `AgentMessageChunk` notification (PRD §16: "Where ACP has no
  dedicated event, VRO events should be translated into existing session
  update or status mechanisms without changing required top-level wire
  fields"). `VroEventSink` is the trait port the orchestrator pushes events
  through; `RecordingVroEventSink` is the test fixture. Errors are
  diagnostic-only — a translation failure must never abort a turn (PRD §17).

## Verification

- Run `cargo test -p vesper-acp --all-features`.
- Run `cargo xtask acp verify` and architecture checks.
- Run the slow-reader and cancellation-under-pressure process tests.

## Child DOX Index

No children.
