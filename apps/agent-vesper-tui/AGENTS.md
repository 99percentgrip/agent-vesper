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
- `src/auth_hub.rs` — pure provider-driven authentication startup state machine and
  responsive masked Ratatui renderer. It may expose only authentication
  descriptors registered by production provider adapters.
- `src/commands.rs` — slash-command parsing, registry, and resolution
  against the active provider's superpowers. Tier C Phase 7 (ADR 0010): the
  registry now covers the **entire** Python oracle `LOCAL_COMMANDS` surface
  (80 distinct oracle command names, including `/export last`, + 3
  Vesper-native = 83 commands). The
  `ORACLE_COMMAND_SURFACE` const table is the single source of truth for the
  migration matrix.
- `src/dispatch.rs` — pure, terminal-free event-loop dispatch: the bridge
  between the command registry, the Plan Mode state machine, and the
  `SuperpowerOverrides` store. Owns `SessionState`, `DispatchOutcome`, and
  `dispatch()`. The full Plan Mode lifecycle is unit-tested here under a
  `StubRenderer`; the binary owns only the crossterm input buffer.
- `src/superpowers.rs` — `ProviderSuperpowerSurface` and
  `SuperpowerOverrides`, the pure projection the TUI keeps of the active
  provider's advertised descriptors.
- `src/ui.rs` — `TerminalRenderer` trait, `ViewModel`, `StubRenderer` for
  tests, and the production `render_to_frame` ratatui/crossterm backend. The
  production view owns the oracle-style conversation/reasoning/sidebar/
  composer composition, full-height slash-command palette, clickable footer,
  working-tree panel, live activity, TODO, and structured run report. The
  Conversation and Reasoning panels render assistant/reasoning text through
  `markdown::render_markdown`; scroll is estimated from the rendered Lines.
- `src/markdown.rs` — self-contained, streaming-safe markdown → ratatui
  `Line` renderer. Re-parses the buffered assistant text every frame so
  partial syntax degrades gracefully: open inline markers (`**bold` with no
  closer) render literally and unclosed fenced code blocks render the
  remainder as a styled code block. Supports bold, italics, inline code,
  fenced code blocks, ordered/unordered lists with nesting, and ATX
  headings. Underscore emphasis is intentionally unsupported so `snake_case`
  identifiers stay intact. Pure, `#![forbid(unsafe_code)]`, no new
  dependency (kept the crate free of an external markdown crate's
  unsafe/MSRV risk).
- `src/mobile.rs` — credential-free bounded HTTP approval companion with
  random pairing/approval capabilities, expiry, malformed-request rejection,
  fail-closed public-bind policy, and QR rendering only for explicitly
  advertised phone-reachable URLs.
- `src/lib.rs` — public re-exports and `query_startup_view`, the single
  integration point between the TUI and the runtime registry.
- `src/main.rs` — binary entry point; crossterm raw-mode + alternate-screen
  lifecycle and the interactive event loop. Delegates every transition to
  `dispatch::dispatch` so it owns no Plan Mode discipline itself. Owns the
  startup credential interception route and performs native credential-store
  calls on Tokio blocking threads before entering the conversation loop. Owns the
  credential-free `RuntimeSupervisor` and drains `SessionState.pending_reasoning`
  into the runtime `UpdateSessionReasoning` command after each dispatch (ADR 0009).
  Phase 6 (ADR 0010): also owns the multi-turn `vesper_agent::AgentLoop` bridge —
  `build_agent_loop`, `spawn_agent_turn` (background `tokio::spawn`), and the
  non-blocking `drain_agent_event` / `apply_agent_event` result handlers.
  Free-text prompts in NORMAL phase spawn the loop; a model-authored plan
  drives `PLANNING → REVIEW` via `dispatch::apply_model_plan`. `TuiSession`
  owns conversation history and receives the updated history from each turn,
  keeping successive prompts in one provider-visible context. The complete
  36-name hosted Python tool surface is advertised by the shared `vesper-harness`
  `ToolService`:
  memory/skills, cron, session-context search, bounded semantic inspection,
  transactional patch sets, batch reads, workflows, signed plugins, and
  provider-backed delegate/worktree workers share the same composition roots.
  Phase 8 (ADR 0011): the shared harness owns the model-facing `MemoryStores`;
  the TUI retains its slash-command projection bundle
  (`MemoryStore` + `SkillStore` + `UserProfile` + `AwarenessLedger`) and the
  `drain_memory_op` executor that turns `SessionState.pending_memory_op`
  into durable reads/writes after each dispatch.
  Phase 9 (ADR 0012): the shared harness owns model-facing cron/session
  services; the TUI owns the slash-command `CheckpointStores` bundle
  (`CheckpointsLedger` + `SessionLineage` + `CronRegistry` +
  `SessionExporter` + `ClipboardPort`; `CiStatusReader` is process-scoped)
  and the `drain_checkpoint_op` executor that turns
  `SessionState.pending_checkpoint_op` into durable snapshots / restores /
  lineage / cron / export / clipboard / CI-status operations after each
  dispatch. The Errno-24-prevention discipline lives entirely in
  `vesper-checkpoints` (RAII file-handle scoping; no SQLite, no git refs).
  Phase 10 (ADR 0013): the shared harness owns model-facing MCP/plugin
  gateways; the TUI owns the slash-command `McpStores` bundle (`McpRegistry` +
  `PluginLoader` + `TrustedPublishers`) and the `drain_mcp_op` executor
  that turns `SessionState.pending_mcp_op` into MCP server list/add/
  remove/tools and plugin list/publishers/verify/load/trust operations.
  The No-Leak Guarantee lives entirely in `vesper-mcp`
  (`#[cfg(debug_assertions)]` gates `load_unsigned_debug`; release builds
  structurally erase the method).

## Local Contracts

- Stdout carries only terminal escapes via crossterm; no ACP/JSON-RPC may
  appear there. Tracing goes to stderr only.
- The crate depends on `vesper-auth`, `vesper-domain`, `vesper-provider`,
  `vesper-provider-glm`, `vesper-runtime`,
  `vesper-agent` (Phase 6 / ADR 0010: the binary composes the multi-turn
  agent loop), `vesper-memory` (Phase 8 / ADR 0011: the binary owns the
  durable memory store bundle), `vesper-checkpoints` (Phase 9 / ADR
  0012: the binary owns the durable checkpoint/session-lineage/cron/
  export/clipboard/CI bundle), and `vesper-mcp` (Phase 10 / ADR 0013:
  the binary owns the durable MCP-registry + Ed25519-signed plugin
  loader bundle), `vesper-sessions` for bounded persisted transcript search,
  and `vesper-observability` for opt-in telemetry, plus `vesper-harness` for
  the shared hosted tool implementation; it must not depend on
  `vesper-acp`, SQLite, or any disposable spike.
  `vesper-provider-synthetic` is dev-only and may never be selected by a
  production binary.
- The Plan Mode state machine is **pure**: no I/O, no async, no global
  state. Every transition returns a `PlanTransition`; the event loop applies
  it.
- Plan Mode reasoning text is produced by the model through the runtime; the
  TUI owns the transition discipline, not the reasoning.
- The crate stays `#![forbid(unsafe_code)]` and respects workspace MSRV
  1.88, workspace lints, and `-D warnings` Clippy.
- ADR 0009: the GLM reasoning surface is the single `/thinking` dial
  (`{disabled, enabled, high, max}`); `/effort` is retired. `dispatch` stays
  pure and produces `SessionState.pending_reasoning` for any resolved
  `zai:reasoning` superpower; the binary's async loop applies it to the
  runtime. The GLM `reasoning_mode_for_superpower` mapper lives in
  `vesper-provider-glm`.
- Superpower commands (`/thinking`, `/model`) are resolved
  dynamically against the active provider's advertised descriptors at
  dispatch time, so the same command surface works for any registered
  provider.
- Mutating agent tools run under the injected one-time `ApprovalBroker`; the
  TUI displays one pending request and resolves it only on `/approve` or
  `/cancel`. A closed channel fails closed. `@file`, `@folder`, `@diff`, and
  `@symbol` references are expanded under the workspace with untrusted
  delimiters and bounded sensitive-file filtering.
- Persisted TUI search uses the bounded `vesper-sessions` linear search port;
  its projection contains only user/assistant text and is atomically
  replaced. SQLite/FTS indexes are intentionally absent.
- `AGENT_VESPER_TELEMETRY` opt-in enables the secret-safe trajectory recorder;
  prompts, tool payloads, reasoning, paths, commands, and credentials are
  excluded from JSONL events.
- Provider selection follows `AGENT_VESPER_PROVIDER` (default `zai`), the
  same composition-boundary convention as `agent-vesper-acp`.
- Missing or locally malformed required credentials route to the Agent
  Vesper Authentication screen before the main loop. Environment credentials retain precedence; new
  stored credentials use the OS credential manager with the documented
  owner-only Unix vault fallback. No live provider call is made by startup
  validation.
- Auth is provider-routed: the `AuthProvider` is projected from each
  provider's advertised `ProviderFactory::descriptor()` (env var via
  `secret_reference_fields[0]`, `key_url`) through the registry and
  `StartupView.auth`. The TUI holds no hardcoded provider match arms. A
  provider-routed `/auth` slash command (`UiAction::OpenAuth` →
  `SessionState.pending_reauth`) re-opens the screen mid-session. Storage
  (`vesper-auth`) and per-adapter resolution are unchanged.

## Work Guidance

- Keep the Plan Mode, command registry, superpower adapter, dispatch surface,
  and renderer trait unit-testable without touching a real terminal — the
  production binary is the only module that may invoke crossterm directly.
- Keep Auth Hub provider choices registry-driven. Do not render aspirational
  providers, models, plans, endpoints, or authentication methods.
- The composer must expose the registered oracle commands while the input
  begins with `/`: the binary owns palette selection/completion key handling,
  while `CommandRegistry::completion_candidates` remains pure and derives its
  labels/descriptions from `ORACLE_COMMAND_SURFACE`. The palette must make the
  complete registry reachable through a scrolling viewport; Enter submits the
  highlighted command, while configurable commands first expand into values
  advertised by the active provider and free-form commands leave the cursor at
  their argument position. Tab completes without submitting.
- All event-loop transition logic lives in `dispatch::dispatch`. When a new
  command or transition is added, extend `CommandOutcome` in `commands.rs`,
  add a `match` arm in `dispatch::apply_outcome`, and cover the lifecycle in
  `dispatch::integration_tests`. The binary's event loop must never grow its
  own transition discipline.
- ADR 0010 (Tier C Phase 5): `/review` is **retired**. The model now drives
  `PLANNING → REVIEW` by emitting the `update_plan` tool; the agent loop
  surfaces the plan (`AgentTurnOutcome::plan`) and the binary calls
  `dispatch::apply_model_plan(body)` to finalize it. The human no longer
  authors the plan body.
- ADR 0010 (Tier C Phase 6): the binary owns the multi-turn agent-loop
  bridge. Free-text prompts in NORMAL phase spawn `AgentLoop::run_prompt` in
  a background `tokio::spawn`; the event loop `try_recv`s the result each
  iteration so the UI stays responsive (a "WORKING..." banner is shown
  in-flight). A `Completed { plan: Some(body), .. }` outcome routes through
  `dispatch::apply_model_plan`. PLANNING-phase free text stays inline
  (driver answers the pending question); the loop is never spawned there.
  Construction (`build_agent_loop` / `build_agent_config`) is credential-free
  and provider-aware (GLM `zai` / `synthetic`); dispatch fails fast on
  missing credentials or unknown providers.
- ADR 0010 (Tier C Phase 7): 100% command routing parity with the Python
  oracle's `LOCAL_COMMANDS`. Every registered command resolves to a concrete
  typed handler; an accidental missing route fails as an internal parity
  violation. No deferred fallback exists. Workflow commands
  (`/security-review`, `/smart`, `/release`, `/insights`, `/diff`) build a
  prompt and stash it on `SessionState.pending_prompt`; the binary drains it
  into a background `AgentLoop` turn (same path as free-text prompts).
- ADR 0011 (Tier C Phase 8): the 13 awareness/memory commands
  (`/memory`, `/goal`, `/subgoal`, `/skills`, `/profile`, `/awareness`,
  `/metacognition`, `/deliberation`, `/repository`, `/meta-learning`,
  `/observability`, `/curator`, `/journey`) are no longer deferred. They
  resolve to `CommandOutcome::Memory(MemoryOp)`; `dispatch` records
  `SessionState.pending_memory_op`; the binary owns a `MemoryStores`
  bundle (`MemoryStore` + `SkillStore` + `UserProfile` + `AwarenessLedger`
  under `AGENT_VESPER_MEMORY_ROOT` or `.agent-vesper/memory/`) and drains
  the op synchronously after dispatch (these are local filesystem
  reads/writes — fast enough not to block the UI).
- ADR 0012 (Tier C Phase 9): the 13 checkpoint/session/loop/export/copy/ci
  commands (`/sessions-new`, `/sessions`, `/lineage`, `/branch`,
  `/rename`, `/checkpoint`, `/rollback`, `/rewind`, `/undo`, `/loop`,
  `/export`, `/export last`, `/copy`, `/ci`) are no longer deferred. They
  resolve to
  `CommandOutcome::Checkpoint(CheckpointOp)`; `dispatch` records
  `SessionState.pending_checkpoint_op`; the binary owns a
  `CheckpointStores` bundle (`CheckpointsLedger` + `SessionLineage` +
  `CronRegistry` + `SessionExporter` + `ClipboardPort` +
  `CiStatusReader` under `AGENT_VESPER_CHECKPOINT_ROOT` or
  `.agent-vesper/checkpoints/`) and drains the op synchronously after
  dispatch. **Errno 24 prevention:** the `vesper-checkpoints` crate uses
  strict RAII (`Drop`) file-handle discipline — no `File` is ever stored
  in a long-lived struct, no SQLite, no git refs, no auto-snapshotting.
  Checkpoints are explicit-only by structural design.
- ADR 0013 (Tier C Phase 10): the final 2 commands (`/mcp`, `/plugins`)
  are no longer deferred. They resolve to `CommandOutcome::Mcp(McpOp)`;
  `dispatch` records `SessionState.pending_mcp_op`; the binary owns an
  `McpStores` bundle (`McpRegistry` + `PluginLoader` +
  `TrustedPublishers` under `AGENT_VESPER_MCP_ROOT` or
  `.agent-vesper/mcp/`) and drains the op after dispatch. **No-Leak
  Guarantee:** `vesper-mcp`'s unsigned-plugin loading code path is
  structurally erased from `--release` builds via
  `#[cfg(debug_assertions)]`; a release binary cannot load an unsigned
  plugin by any code path. Plugins are declarative only (the
  `executable_code` permission is rejected at validation time). With
  Phase 10 shipped. The former composer, live-settings, image, sound, mobile,
  keybinding, accessibility, Vim, and terminal-integration exclusions are now
  concrete native operations. Tests iterate the complete registry and reject
  any hidden missing route.
- Footer and palette rows are mouse-operable while TUI mouse capture is active.
  F4 cycles bounded real Changes/Git/Diff/Files/GitHub views. F5 uses the same
  optional `arecord`/`afrecord` plus local `faster-whisper` contract as the
  frozen oracle and must report unavailable dependencies without fabricating
  input. Ctrl-Shift-C copies only app-managed mouse-selected transcript text.
- Provider catalogs and provider-specific settings belong to adapters. The
  production composition currently registers only the real Z.ai adapter;
  provider-neutral runtime/registry boundaries must not be described as a
  second production provider.
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
- Run `cargo test -p agent-vesper-tui --bins` (Phase 6 wiring:
  provider-aware config, `build_agent_loop`/`build_agent_config`, the
  `AgentEvent → SessionState` mapper, and the spawn/drain plumbing).
- Run `cargo clippy -p agent-vesper-tui --all-targets --all-features -- -D warnings`.
- Run `cargo run --package xtask --quiet -- architecture` (the TUI must
  appear in the validated package count and pass the dependency-direction
  gate, including the new `agent-vesper-tui → vesper-agent` edge).
- Run `cargo build -p agent-vesper-tui --bins` to confirm the binary
  links under the workspace toolchain.

## Supply-chain note

The crate pins `ratatui = "=0.30.2"` and `crossterm = "=0.29.0"` together:
- ratatui 0.29.0 pulled in `paste 1.0.15` (RUSTSEC-2024-0436 — unmaintained)
  and `lru 0.12.5` (RUSTSEC-2026-0002 — unsound `IterMut`). ratatui 0.30.2
  dropped `paste` entirely and moved to `lru 0.18.1`, eliminating both
  advisories without ignoring them.
- crossterm must be `=0.29.0` (not `=0.28.1`) so the workspace, the TUI,
  and `ratatui-crossterm 0.1.2` all share one crossterm version. The
  workspace pin keeps `default-features = false` for the minimal surface
  but explicitly enables the `windows` feature, because crossterm gates
  the `winapi`/`crossterm_winapi` backend deps behind that feature —
  without it the crate fails to compile on `x86_64-pc-windows-msvc` with
  E0432/E0433 (`unresolved import crossterm_winapi`, `cannot find module
  winapi`). The Windows-only deps are target-gated so enabling `windows`
  on Linux/macOS pulls in nothing.

Do not downgrade ratatui or crossterm, and do not drop the `windows`
feature, without re-running `cargo deny check`, `cargo audit`, and the
five-target CI matrix.

## Child DOX Index

No children.
