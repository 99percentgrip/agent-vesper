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
- `src/lmstudio_hub.rs` — pure LM Studio provider settings state machine +
  Ratatui renderer + atomic JSON persistence (`/lmstudio`). Mirrors the
  `auth_hub` pattern: the user adjusts the LAN/localhost `api_base_url` and
  optional pinned model **inside the TUI** (not a config file); the binary owns
  the terminal event loop and persists on `Save`. The settings file holds only
  non-secret fields (`$AGENT_VESPER_LMSTUDIO_ROOT` or `.agent-vesper/lmstudio/
  settings.json`); the optional API key is read from the `LMSTUDIO_API_KEY`
  env var (surfaced as a screen hint) — moving it to the OS credential store is
  the security follow-up.
- `src/commands.rs` — slash-command parsing, registry, and resolution
  against the active provider's superpowers. Tier C Phase 7 (ADR 0010): the
  registry now covers the **entire** Python oracle `LOCAL_COMMANDS` surface
  (80 distinct oracle command names, including `/export last`, + 16
  Vesper-native = 96 commands; Vesper-native = approve, cancel, auth,
  lmstudio, provider, embedding, chat-only, quit, remember, recall, forget,
  memories, promote, demote, interview-limit, firewall, sandbox). The
  `ORACLE_COMMAND_SURFACE` const table is the single source of truth for the
  migration matrix. `chat-only` (the `/chat-only` palette twin of the F11
  keybinding) resolves to `UiAction::ToggleChatOnly`; like every registry
  entry it keeps `quit` last so the palette order contract holds. ADR 0016 follow-up: `/embedding` (Status/Set/Clear) is
  the most recent Vesper-native addition; it parses
  `key=value` pairs via `EmbeddingPairs::parse` and drains through
  `pending_embedding_op` → `drain_embedding_op` (write `embedding.json` +
  hot-reload + background probe of the new endpoint).
  VRO-8 (PRD §8.1): `/reasoning set mode=<auto|fast|balanced|deep|maximum|`
  off>` overrides the deterministic TaskProfiler; `/reasoning clear` and
  `set mode=auto` clear it; bare `/reasoning` reports the current override.
  The new arm is gated by `set mode=` / `clear` / empty-arg patterns so the
  oracle's existing `/reasoning <level>` superpower alias (resolves to the
  `thinking` descriptor) is fully preserved for any other argument shape.
  Resolves to `CommandOutcome::ReasoningOverride { mode }` /
  `CommandOutcome::ReasoningStatus`; `parse_reasoning_mode` is the
  six-variant parser (PRD §8.1 authoritative list + `max` shorthand for
  `Maximum`; rejects every invented mode with a usage error listing all
  six).
  ADR 0019: `/interview-limit` reports or changes the session-scoped
  VesperLens question policy. Bare reports; `auto` lets the agent choose
  1–12 decision-relevant questions; `1`–`12` sets a hard maximum. The default
  remains 4. The command, palette, tool schema, and executor share one typed
  policy, and the schema is rebuilt for every turn so the model sees the
  active value.
  VRO-13 PR-4: `/sandbox [on|off|status]` resolves to
  `CommandOutcome::ContextView(ViewKind::Sandbox)` (bare or `status`) or
  `CommandOutcome::SandboxControl(SandboxControl::On|Off)`. The route is
  boot-resolved (once-only holder), so `on`/`off` answer with the honest
  edit-config-and-restart instruction — byte-identical text to the ACP
  host's `/sandbox` surface (host-parity contract); unknown arguments get
  the shared usage error. `/status` surfaces `sandbox=on|off` alongside the
  firewall line.
  ADR 0021: cognitive memory is composed as two independent engines. The
  existing project store remains at `AGENT_VESPER_COGNITION_ROOT` or
  `.agent-vesper/cognition/`; the global store uses
  `AGENT_VESPER_GLOBAL_COGNITION_ROOT`, then
  `$XDG_DATA_HOME/agent-vesper/cognition/`, then
  `~/.local/share/agent-vesper/cognition/`. `/remember` smart-routes stable
  identity/preferences globally and repository facts locally, conservatively
  defaults ambiguous text to the project, accepts `--global` / `--project`
  (`--local`) overrides, and always echoes the chosen scope and reason.
  `/recall` and automatic recall search both stores, `/memories` audits them,
  and `/promote` / `/demote` move a short- or full-ID memory between them.
- `src/dispatch.rs` — pure, terminal-free event-loop dispatch: the bridge
  between the command registry, the Plan Mode state machine, and the
  `SuperpowerOverrides` store. Owns `SessionState`, `DispatchOutcome`, and
  `dispatch()`. The full Plan Mode lifecycle is unit-tested here under a
  `StubRenderer`; the binary owns only the crossterm input buffer.
  VRO-8 (PRD §8.1): `SessionState.reasoning_mode_override` holds the
  manual override; `SessionState::effective_reasoning_mode()` is the single
  function the binary consults before routing a VRO turn (returns the
  override, or `Auto` when none is set or when the override is itself
  `Auto` — both mean "profiler decides"). The dispatcher's
  `ReasoningOverride` arm normalizes `Auto` to `None` so the profiler is
  back in charge; `ReasoningStatus` reads the live override and surfaces it
  in the status line.
- `src/superpowers.rs` — `ProviderSuperpowerSurface` and
  `SuperpowerOverrides`, the pure projection the TUI keeps of the active
  provider's advertised descriptors.
- `src/capabilities.rs` — fail-closed per-model capability index
  (`ModelCapabilityIndex` + `CapabilityDenial`) over the active provider's
  catalog snapshot (PRD `docs/provider-capability-gating-prd.md`). Gates
  image input (`accepts_image`), tool/adviser eligibility
  (`adviser_candidates`), advertised reasoning levels, and exact context
  limits. `Unknown`, missing models, and empty advertised media-type lists
  deny with provider-neutral reasons mirroring
  `vesper_provider::resolve_support` (`Unknown` + `Require` → `Reject`).
- `src/ui.rs` — `TerminalRenderer` trait, `ViewModel`, `StubRenderer` for
  tests, and the production `render_to_frame` ratatui/crossterm backend.
  **Current layout:** the bottom Reasoning panel and Activity strip stay
  removed; the Conversation column owns chat, inline thinking, and tool
  telemetry. New sessions default to chat-only full width. F11 reveals a
  compact right rail on wide terminals with Session, a
  dedicated live TODO panel, and a bounded Last run summary. `/tasks`
  toggles the TODO region and reveals the sidebar when enabling it. F11
  (`toggle_chat_only`) collapses the entire right rail — Session + TODO +
  Last run — into a chat-only full-width view; the collapse is a pure
  render-time overlay (`PanelVisibility::chat_only`), so the per-panel
  `sidebar` / `tasks` flags keep their values and a second F11 restores the
  exact previous layout. The conversation is a borderless padded canvas, the
  composer uses one quiet top divider, and the optional utility rail uses one
  vertical divider with flat section headings rather than stacked rounded
  boxes. `/tasks` and `/statusline` clear the overlay when
  they explicitly reveal the rail. The footer advertises the F11 action
  beside the other mouse-operable footer segments. The provider
  chain of thought streams INLINE in the Conversation feed:
  `transcript_lines_for` emits a `thinking:`-prefixed block (compact
  `ReasoningDiagnostics::render_inline_header()` label + the newest
  `INLINE_THINKING_TAIL_LINES` reasoning lines) while a turn runs;
  `render_transcript_lines` renders `thinking:` entries dim + italic. Raw
  `⏺`/`⎿` telemetry is collapsed by default into one categorized run summary;
  Ctrl+T toggles the complete activity transcript during or after the turn
  without replacing the final answer. Review URLs remain visible in compact
  chat. `PanelVisibility`
  now means: `reasoning` = inline-thinking visibility (F2), `sidebar` =
  right-rail visibility, and `tasks` = dedicated TODO visibility. `ViewModel` no longer carries
  `reasoning_manual_scroll` / `reasoning_panel_focused` — every scroll
  input targets the conversation. Tool telemetry uses the `⏺` action /
  `⎿` result glyphs (Claude Code parity; the strings are formatted in
  `main.rs::apply_agent_progress`).
  User turns (`user:` prefix) render as plain markdown with a compact cyan
  `›` prompt marker; assistant turns remain unboxed. Legacy full-width and
  asymmetric chat-bubble backgrounds are prohibited. Consecutive thinking
  and expanded tool action/result entries form one compact activity group
  without blank rows between every event; human turns retain breathing room. Submitted
  bracketed pastes remain compact `[Pasted Content N chars]` chips in the
  visible transcript while their complete text still reaches provider history.
  Mouse selection starts and ends only inside the Conversation column; sidebar
  and lower-chrome hits never become transcript selections. The interactive
  Pending vision images render as numbered `[Image #N]` attachment chips at
  the start of the Composer, matching Codex/Claude attachment UX; queuing an
  image must not add a synthetic line to conversation history. Backspace at
  the start of the editable text removes the last pending image. The chips
  are a render-only projection of the existing `pending_images` payload and
  disappear when that payload is consumed by submission.
  Multiline or 256+-character text pastes follow the same compact UX: retain
  the exact payload outside the editable line, render
  `[Pasted Content N chars]`, expand it only when Enter submits the prompt,
  and let Backspace at the editable-text origin remove the newest paste chip.
  Short single-line pastes remain directly editable.
  While an agent turn runs, slash-command results remain foreground-visible
  after the live region and asynchronous `/usage` uses its independent
  channel. Bare `/permission` reports the active mode; explicit
  `ask|read|bypass` values mutate it. During any agent or VRO turn, free-text
  Enter steers that same turn through a host-owned inbox drained at the next
  safe model boundary; it never aborts the active provider stream or tool.
  Tab submits a distinct visible FIFO follow-up that starts after the active
  turn. Ctrl+C remains the explicit cancellation path and preserves already-
  visible assistant/tool output. The activity strip reports the queue count
  and both non-cancelling gestures.
  `render_permission_modal` overlays a centered `Clear` + bordered dialog
  (`PermissionModal`/`PermissionChoice` exported from `lib.rs`) whenever
  `ViewModel::pending_permission` is set; the binary's event loop intercepts
  Tab/Left/Right (toggle focus) and Enter/Esc (submit/dismiss) while the
  modal is up and resolves through `PermissionRequest::approve` / `reject`.
  VRO-8 (PRD §8.1): `ReasoningDiagnostics` is a label-typed struct (snake_case
  strategy + kebab-case mode + lowercase risk + numeric budget fields)
  exposed via `lib.rs`. Since VRO-11.5 it renders as the ONE-line inline
  thinking header (`render_inline_header()`) — phase · strategy · mode
  (with `(override)` when the user forced it) · risk, plus a prominent
  **⚠ RISK ESCALATION** marker when risk escalated to `High`. The full
  markdown budget header (`render_header()`) remains available for hosts
  that want Depth / Branches / Models / Repairs. The binary populates
  `ViewModel.reasoning_diagnostics` before each VRO turn; `None` (the
  default) renders a plain `🧠 Thinking…` header.
- Live agent progress and terminal completion share one FIFO per-turn mpsc
  channel. Reasoning/content deltas must remain ordered, and terminal
  finalization must replace the visible streaming region with exactly one
  transcript copy of the assistant answer; never spawn independent
  per-delta delivery tasks.
- Shared `AgentTurnOutcome::Interrupted` is rendered and recorded as an
  explicit interrupted terminal while preserving partial assistant content,
  current plan, and conversation history; it must not become a generic failure
  or ordinary completion.
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
- `src/lmstudio_provider.rs` — LM Studio runtime provider adapter
  (composition boundary, VRO-3.x): the `LmStudioFactory` /
  `LmStudioSession` wires the local/LAN model server as a real runtime
  provider (`ProviderFactory`, `ModelCatalog`, `ProviderSuperpowers`,
  `ProviderCredentialPort`), so it appears in `/provider` selection,
  `/model` lists the server's models, and chat dispatches through the
  standard `AgentLoop` (SSE streaming with reasoning-content telemetry for
  Qwen3/DeepSeek-R1-style local reasoning models). The binary owns the
  `reqwest::Client`; no foundational crate imports HTTP.
  PRD `provider-capability-gating` P5: the catalog fetches the verified
  native `GET /api/v1/models` schema (lmstudio-ai docs
  `1_developer/2_rest/list.md` — evidence in the PRD) into a shared cache;
  `ProviderCapabilities` are mapped ONLY from reported fields (vision /
  trained_for_tool_use / reasoning.allowed_options / max_context_length),
  unreported fields stay `Unknown` (fail-closed), and embedding models are
  skipped. Advertised superpowers derive from the cache: the model selector
  lists cached LLMs, and a thinking dial appears ONLY when the pinned model
  reports `reasoning.allowed_options` with those exact labels — the former
  unconditional `disabled/enabled/high` dial never reached the wire and is
  removed (an unbacked control is worse than an absent one).
  VRO-5.3 also ships [`ReqwestLmStudioTransport`] — a `reqwest::Client`-backed
  implementation of the `LmStudioTransport` trait port that the VRO
  `LmStudioReactAgent` uses for `next_action` calls. This is the
  composition-boundary HTTP seam for the Tool-Grounded ReAct loop; it
  mirrors the existing `LmStudioSession` request path (same 120s timeout,
  same header-map helper) and is constructed credential-free.
- `src/main.rs` — binary entry point; crossterm raw-mode + alternate-screen
  lifecycle and the interactive event loop. Delegates every transition to
  `dispatch::dispatch` so it owns no Plan Mode discipline itself. Owns the
  startup credential interception route and performs native credential-store
  calls on Tokio blocking threads before entering the conversation loop. Owns the
  credential-free `RuntimeSupervisor` and drains `SessionState.pending_reasoning`
  into the runtime `UpdateSessionReasoning` command after each dispatch (ADR 0009).
  Owns the skill-store global read layer wiring: `MemoryStores::open_default`
  roots `SkillStore` at `AGENT_VESPER_GLOBAL_MEMORY_ROOT` (default
  `~/.agent-vesper/memory`, resolved via `USERPROFILE`/`HOME`; missing root
  disables the layer). Owns the hosted skill tool surface: `read_skill`
  accepts optional `section`/`offset`/`limit`; `learn_skill` writes
  frontmatter (name/description + optional `environments`/`requires_tools`/
  `tasks`) with oracle-bounded inputs (500/12,000 chars).
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
  VRO-5.3 (PRD §11.6): wires the Tool-Grounded ReAct loop into the
  composition boundary. The dispatch block in `drive_loop` profiles each
  prompt; when the strategy is `ToolGroundedReact` AND a real
  `LmStudioReactAgent` bundle is available (LM Studio settings configured),
  it routes through `spawn_vro_react_turn` → `VroOrchestrator::execute_react`
  (live tool-grounded loop) instead of `spawn_vro_turn` → `execute` (the
  GVR baseline). The decision is factored into the pure
  `react_dispatch_for(strategy, react_available)` helper for
  unit-testability. `build_vro_react_bundle` constructs the
  `LmStudioReactAgent` (from persisted LM Studio settings +
  `LMSTUDIO_API_KEY` env) and a `RegistryToolInvoker` over a fresh
  `ToolRegistry::parity_default()` (sharing the same `TuiToolService` Arc
  as the direct path) plus the same shared `ApprovalBroker` Arc, so the
  React path honors the same hosted-tool surface and one-time approval
  semantics as the direct `AgentLoop`. The agent_tools + approval_port Arcs
  are cloned in `run()` BEFORE they are moved into the AgentLoop and passed
  to `drive_loop` for this purpose.
  Live trajectory rendering (directive 3): both the `ReactAgent` and the
  `ToolInvoker` are wrapped in `TrajectoryCapturingReactAgent` /
  `TrajectoryCapturingInvoker` decorators that share one
  `mpsc::UnboundedSender<String>` and stream each Action/Observation/Finish
  as a pre-formatted markdown line. The event loop drains the receiver via
  `drain_trajectory(session)` each iteration (alongside
  `drain_agent_event`) and appends to `session.live_trajectory` (VRO-11.4),
  which the transcript renderer surfaces INLINE in the Conversation panel
  as the loop runs. The per-entry formatters (`format_react_action_entry` /
  `format_react_observation_entry` / `format_react_finish_entry`) are the
  live path; `format_react_trajectory` is the canonical bulk-render utility
  (exercised by tests, reserved for future bulk-render use cases).
  VRO-11.3 (UX Hotfix): four surgical TUI patches closing the dashboard-test
  gaps. **(1) Bracketed Paste Mode** — `enter_raw_mode` /
  `leave_raw_mode` queue `EnableBracketedPaste` / `DisableBracketedPaste`
  alongside the existing mouse-capture commands, and the main event loop
  handles `Event::Paste(text)` as a single contiguous insertion at the
  composer cursor (NOT as individual `Char` / `Enter` events, which would
  shatter multi-line clipboard content into premature submissions on the
  first embedded `\n`). The paste is swallowed while the permission modal
  is up so the user cannot type behind the dialog. **(2) Live Tool
  Telemetry** — `format_react_executing_entry(name)` emits
  `⏳ *Executing* \`<name>\`...` to the trajectory channel BEFORE
  `TrajectoryCapturingInvoker` awaits `inner.invoke`, so the Reasoning
  panel mirrors Codex / Claude Code's "the agent is acting" affordance
  instead of freezing during a slow tool call; the matching Observation /
  Error line streams second. **(3) Autocomplete Disconnect** — the
  `/reasoning` argument surface is no longer aliased to `/thinking` in the
  palette UI. `command_palette_candidates` short-circuits `/reasoning`
  through the pure `reasoning_argument_candidates` helper, which surfaces
  the six PRD §8.1 VRO modes (`set mode=auto|fast|balanced|deep|maximum|off`)
  + `clear` instead of the GLM thinking-style levels (`disabled`/`enabled`/
  `high`/`max`). The legacy `"/reasoning" => "thinking"` match arm in the
  fallback is removed. The `ORACLE_COMMAND_SURFACE` description for
  `reasoning` and the `help_text` line both drop the "Alias for /thinking"
  text. The BACKEND `superpower_alias("reasoning") => "thinking"` fall-through
  for `/reasoning <level>` is intentionally preserved (README-documented
  backward compat) — only the autocomplete surface changes. **(4)
  VesperLens file-save interceptor** — see `lens_integration.rs` in
  `vesper-agent`; the TUI's `execute_react` call site inherits the
  interceptor transparently because `VroOrchestrator::execute_react` wraps
  its `invoker` argument with `LensObservingInvoker` when a `LensReviewPort`
  is configured (zero-cost when not).
  **VRO-11.4 (Local Recon & UX Overhaul)**: four architectural course-
  corrections driven by architectural analysis.
  **(1) Collapsible Inline Telemetry** — tool execution logs are owned by the
  main Conversation canvas rather than the Reasoning sidebar, but normal chat
  projects them as a single `Ran N tools` summary. Ctrl+T exposes the raw log
  and works after completion, preventing either telemetry floods or lost run
  history.
  A new `TuiSession.live_trajectory: Vec<String>` field collects per-turn
  tool telemetry from both the direct path (`AgentProgressEvent::ToolStarted`
  / `ToolFinished` → `> 🛠️ Executing: <name>...` / `> ✓ <name>`) and the
  ReAct trajectory stream (`drain_trajectory` → `> ⏳ *Executing* ...`).
  The ViewModel's `transcript_lines_for` owns both compact and expanded
  projections. The field is cleared only when a new turn starts alongside
  `reasoning`, so the completed run remains inspectable.
  **(2) Explicit `request_human_review` tool** — the implicit
  `LensObservingInvoker` (VRO-11.3 directive 4) is DELETED. VesperLens
  review is now triggered by an EXPLICIT tool the model calls when it wants
  human review, matching the explicit-invocation pattern (explicit CLI
  invocation, no magic interception). The `TuiToolService` gains an
  optional `lens_review: Option<Arc<dyn LensReviewPort>>` + `lens_url_tx`
  channel. When configured, `definitions()` advertises
  `request_human_review(file_path)` (ReadOnly, blocks until the human
  submits). The tool confines an HTML file to the primary workspace, routes it
  through `LensReviewPort::review_file`, and returns `feedback_as_context_message` as
  the tool result. The `on_url` callback sends the review URL through the
  channel → `drain_lens_urls` → `live_trajectory` so the user sees
  `[VesperLens] Artifact ready for review. Open: <URL>` inline in the
  Conversation panel. **(3) Explicit ownership** — the lens port is always
  constructed for `TuiToolService`; ADR 0020 removed the dormant
  `VroOrchestrator` final-output seam. **(4)
  `LensReviewPort` trait signature** — `on_url` is now tied to the `'a`
  lifetime of `&self` (was elided) so concrete impls like `VesperLensPort`
  can call `on_url` from within the returned async block (needed because
  `VesperLens::review_artifact` calls `on_url` mid-async when the TCP
  listener binds).
  Zero-breakage: when LM Studio is NOT configured or the strategy is
  anything other than `ToolGroundedReact`, the existing direct / GVR /
  parallel-candidates paths are completely unchanged.
  **VRO-11.5 (Claude Code UI & prompt enforcement)**: (1) **tool-execution
  enforcement** — `build_agent_loop` now ALWAYS appends the
  `tool_enforcement_instruction()` system instruction (after project
  instructions + the optional cognition instruction): artifact-generation
  requests MUST execute `write_file` within the same turn;
  `request_human_review` is conditional, workspace-confined, and HTML-only;
  plan-only yielding is forbidden; Plan
  mode keeps the `update_plan` carve-out. Every path sharing the loop
  (direct, GVR, parallel candidates, tree search, PCA) sees the mandate —
  this is the behavioral patch for the 180s zero-tool turn. (2) **telemetry
  glyphs** — `apply_agent_progress` formats `ToolStarted` as `> ⏺ <name>`
  and `ToolFinished` as `> ⎿ ✓/✗ <name>` (Claude Code parity). (3) **input
  wiring** — Tab with an empty composer is a no-op (the panel-focus toggle
  died with the Reasoning panel); PageUp/PageDown/Home/End and the mouse
  wheel always scroll the conversation; F2 / `toggle_thinking` toggles the
  inline-thinking visibility carried by `panels.reasoning`.
  **VRO-11.6 (review UX parity)**: (1) telemetry uses the exact Claude
  Code shapes — `⏺ <tool>` action (flush-left) / `  ⎿ ✓/✗ <tool>` result
  (indented); the `> ` quote prefix is gone and `drain_trajectory` pushes
  entries AS-IS; the ReAct formatters (`format_react_*`) emit the same
  glyphs. (2) The VesperLens URL announcement sends TWO lines — the
  `[VesperLens] …` message and the **bare URL on its own line** (own-line
  URLs are what terminals auto-linkify; wrapped mid-sentence URLs are
  not); `drain_lens_urls` stashes it on `TuiSession.last_lens_url` and
  sets a "Ctrl+O opens it" status hint. **Ctrl+O** (`open_last_lens_review`
  + pure `lens_opener_command`: `xdg-open` / macOS `open` / Windows
  `cmd /C start`) is the guaranteed browser opener; failures surface the
  copyable URL in the status line. (3) `ui.rs` renders `⏺`/indented-`⎿`
  lines dim and bare-URL lines cyan + underlined (link affordance).
  **VRO-11.7 (clickability + TODO restore)**: (1) `enter_raw_mode(enable_mouse)`
  honors the `native_mouse` preference at every call site. (2) **single URL** — the `on_url`
  announcement no longer embeds the URL inside the message line (v0.20.36
  rendered it twice, neither clickable); it sends one message line plus
  ONE bare-URL line. The later VRO-11.11 layout contract moves live TODO
  state out of transcript history and into the dedicated sidebar panel.
  **VRO-11.9 (wheel + click parity)**: the 11.7 OFF-default killed the
  mouse wheel — in the alternate screen terminals deliver NO wheel events
  to apps unless mouse reporting is enabled (PageUp/Down kept working
  because they are key events). `native_mouse` is therefore **ON by
  default again**, and clickability moved INTO the app: `drive_loop`
  stashes each frame's `ViewModel` on `TuiSession.last_model`;
  `handle_mouse_click` reconstructs the transcript `Rect` (header 1 /
  bottom chrome menu+6 / working-tree 10 / sidebar width split) and calls
  the pure `ui::bare_url_entry_at_row` — an inverse mapping through the
  same render+wrap pipeline — so a click on a bare-URL line opens the
  browser via `open_url_in_browser` regardless of terminal link support.
  Ctrl+O remains. **Browser-stdio isolation**: `lens_opener_command`
  attaches `Stdio::null()` to stdin/stdout/stderr — the launched
  browser's own stderr (Chromium `atom_cache` / GCM `DEPRECATED_ENDPOINT`
  lines) previously inherited the TUI's stdio and sprayed over the
  alternate screen, corrupting the display on Ctrl+O.
  **VRO-11.8**: (1) `AgentProgressEvent::ToolStarted` carries a `hint`
  and `ToolFinished` a `note` — derived by the pure
  `vesper_agent::tool_arg_hint` (whitelisted arg keys only:
  path/pattern/command/…, 48-char cap; NEVER content/body/credential
  keys) and `tool_result_note` (success = size digest "N lines"/"N chars",
  failure = first line of the harness error, 72-char cap) — so telemetry
  renders rich (`⏺ write_file · dashboard.html` /
  `  ⎿ ✓ write_file · 43 lines`) without leaking payloads. (2) The
  enforcement instruction now mandates `update_plan` TODO tracking for
  multi-step tasks in EVERY mode (the Plan-mode-exception wording
  discouraged Code-mode plans — the live-test root cause of the missing
  TODO block). (3) ADR 0020 supersedes the same-document watchdog with trusted
  outer chrome and a sandbox-only annotation SDK, so artifact DOM rebuilds
  cannot remove or impersonate verdict controls.
  **VRO-11.11 (interactive handoff + planning interview):** VesperLens
  review URLs now open in the system browser automatically; the bare URL,
  click handling, and Ctrl+O remain fallbacks. Artifact review starts in
  interaction mode so native page controls work; annotation capture is an
  explicit toggle, `Action::Modify` has a real Send changes action, draft
  notes survive panel rerenders, and non-2xx feedback submissions fail
  visibly. The TUI advertises `request_human_input(title, questions)` beside
  `request_human_review`: it renders 1–12 escaped free-text/radio/checkbox
  questions under the active `/interview-limit` policy, requires every
  answer, blocks on the same loopback Lens port,
  and returns stable question/value pairs as tool context. TODO snapshots no
  longer enter chat history; the dedicated sidebar panel owns current plan
  state and the former full-height Run gauge is a compact status/report.
  **ADR 0020 review hardening:** trusted outer chrome owns verdicts; artifacts
  run in a no-same-origin iframe, feedback is session-authenticated, repeated
  file rounds reuse one live URL, sibling assets are confined, drafts survive
  reload, and annotations include exact range metadata plus editable suggested
  HTML. Layout warnings are passive and reviewer-selected. Both checked-in
  Playwright flows are required release evidence.
  VRO-8 (PRD §8.1 — UX & Diagnostics): three pure helpers + the wiring
  that surfaces VRO telemetry to the driver. (1) `compute_reasoning_diagnostics`
  reads only `VroOrchestrator::profile` (deterministic, allocation-only) +
  `ReasoningBudget::for_mode`; it never calls `execute*`, never mutates the
  orchestrator, and never names a concrete provider. It honors
  `SessionState.reasoning_mode_override` so a `/reasoning set mode=deep`
  override is reflected in the panel header **before** the next turn runs.
  The result is stashed on `TuiSession.reasoning_diagnostics` and projected
  into `ViewModel.reasoning_diagnostics` each frame. (2) `strategy_snake_case` /
  `mode_label_kebab` / `risk_label_lowercase` map the domain enums to the
  exact PRD §10.3 / §8.1 / §14.2 wire labels so the panel header matches the
  JSON shape byte-for-byte. (3) `format_learning_extraction_notice` renders
  the **✓ LEARNED** notice pushed through the trajectory channel after a
  successful VRO turn — **symmetric across both spawn paths** (audit fix):
  `spawn_vro_react_turn` (ReAct) and `spawn_vro_turn` (GVR / parallel
  candidates / tree search / PCA) both emit it when the outcome status is
  `Succeeded` and at least one model call was issued. It is purely
  presentational; the actual VRO-7 procedural-memory persistence happens in
  `VroOrchestrator::execute_with_learning`, which is unchanged. The override
  is honored in three wiring sites: the `should_vro` route check uses
  `effective_reasoning_mode()` (so `Off` routes through the direct
  `AgentLoop`); both VRO request constructors
  (`spawn_vro_turn`, `spawn_vro_react_turn`) set `ReasoningRequest.mode` to
  the effective mode so the orchestrator's budget preset matches the
  user's choice.

## Local Contracts

- Native plans share the agent loop's four-segment bounded autonomous
  continuation with ACP. Each submitted turn seeds the loop from the retained
  task panel, so a resume turn cannot accept acknowledgement text as completion
  while older plan items remain open. If the ultimate cap is reached, status,
  transcript, telemetry, and worker rendering must identify the stop rather
  than imply completion.
- The optional per-turn iteration cap defaults to disabled. `/max-iterations
  enable` restores 50, `/max-iterations disable` removes the user cap, and an
  explicit `1-1000` sets it; none of these removes the ultimate safety ceiling.

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
- PRD `provider-capability-gating` — **dynamic capability gating**:
  provider feature controls are advertisement- and capability-driven, never
  name-checked. `/settings` rows and value palettes derive from the active
  provider's advertised `SuperpowerDescriptor`s (by `command_alias`),
  narrowed by its `SuperpowerPolicy::valid_choices` for the active plan +
  model; a provider that does not advertise a control hides it. Image
  paste, mixture-of-agents advisers, and auxiliary eligibility consult the
  session's `ModelCapabilityIndex` (built at the composition boundary for
  the active provider) and fail closed. No frontend path may call a
  concrete provider's catalog or match on a provider id; concrete adapters
  appear only in composition wiring (`register_default_providers`,
  `provider_configuration_for`, `capability_index_for`).
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
- Host-neutral command parity is declared by
  `vesper_domain::HOST_PARITY_SLASH_COMMANDS`; the TUI registry test must
  contain every shared entry. `/embedding set` replaces the live adapter and
  migrates vectors in both hosts; probing an unused adapter is not sufficient.
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
- Clipboard image paste is an interactive-terminal-only host capability, so it
  has no ACP protocol twin. Plain Ctrl+V reads native bitmap data through the
  platform clipboard and normalizes it to PNG; terminal bracketed paste and
  clipboard text share one path-aware ingestion route so an existing
  PNG/JPEG/WebP/AVIF path is queued through the established image pipeline
  instead of being parsed as a slash command. Undocumented AVIF upload is
  prohibited: pasted AVIF files are normalized locally with ImageMagick or
  ffmpeg, while copied bitmap pixels need no external converter. Normal text,
  multiline text, and real slash commands remain composer input.
- If preserved composer/history content exceeds the active model's catalog
  capability, the TUI offers at most three same-provider, active-plan models
  with Up/Down/Enter/Esc consent. Confirming uses the existing validated
  session model-update path, then dispatches the untouched text and images;
  cancellation never silently switches or drops the composition.
- Footer and palette rows are mouse-operable while TUI mouse capture is active.
  F4 cycles bounded real Changes/Git/Diff/Files/GitHub views. F5 uses the same
  optional `arecord`/`afrecord` plus local `faster-whisper` contract as the
  frozen oracle and must report unavailable dependencies without fabricating
  input. The voice sidecar's Python interpreter is **auto-discovered** with no
  user configuration: the binary probes, in order, an explicit env override
  (`VESPER_PYTHON_PATH` → absolute Python path; `GLM_VENV_PATH` → virtualenv
  root with `bin/python` appended), then the harness-owned voice venv at
  `$AGENT_VESPER_VOICE_VENV`/`$XDG_DATA_HOME/agent-vesper/voice-venv`/
  `~/.local/share/agent-vesper/voice-venv`, then sibling project venvs under
  `$HOME/Projects/*/{.venv,venv,.virtualenv}/bin/python` (alphabetical), then
  bare `python3`. Each candidate is probed with `import faster_whisper`; the
  first success is cached for the process lifetime. When **no** candidate
  works on first F5, the binary **auto-bootstraps** the harness-owned voice
  venv (`uv venv` + `uv pip install faster-whisper`, falling back to
  `python3 -m venv` + `pip install`) and asks the user to press F5 again; this
  makes voice work for any installer user with no separate setup. The `uv`
  used is the **installer-bundled** binary at the bundle dir
  (`$AGENT_VESPER_BUNDLE_DIR` / `$XDG_DATA_HOME/agent-vesper` /
  `~/.local/share/agent-vesper/uv`) preferred over system `uv`; with the
  bundled `uv` present, venv creation needs no external toolchain (the
  `python3 -m venv` fallback still requires `python3`+`python3-venv` and is
  only reached if `uv` is absent). Transcription uses a **long-lived Python
  sidecar** (`VoiceSidecar`): one process loads the `faster-whisper` model
  once on first F5 START (hidden behind recording time) and stays warm for the
  session, transcribing subsequent clips without reloading the model — this is
  what makes push-to-talk feel instant after the first press (the old per-call
  subprocess reloaded the model every time). The sidecar is killed on session
  exit. The
  bootstrap result is cached so subsequent presses are instant. When all
  strategies fail, the status line names the fix. Ctrl-Shift-C copies only
  app-managed mouse-selected transcript text.
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
- VRO-8 (PRD §8.1 — UX & Diagnostics): the manual reasoning-mode override
  + diagnostic telemetry layer. **Contract**: a manual
  `/reasoning set mode=<X>` overrides the deterministic `TaskProfiler` for
  every subsequent VRO turn; `/reasoning clear` or `set mode=auto` returns
  to profiler defaults; bare `/reasoning` reports the current override in
  the status line. **Zero orchestrator breakage**: the TUI computes its
  own diagnostics projection (`compute_reasoning_diagnostics`) from
  `VroOrchestrator::profile` + `ReasoningBudget::for_mode`; it never calls
  `execute*`, never mutates the orchestrator, and never names a concrete
  provider. **Wiring invariant**: every VRO turn dispatch site
  (`should_vro` route check, `spawn_vro_turn`, `spawn_vro_react_turn`) must
  consult `SessionState::effective_reasoning_mode()` so the override is
  honored consistently across the direct path and both VRO paths. When a
  new dispatch site is added, route its mode through the same helper. The
  PRD §8.1 mode list is authoritative — `parse_reasoning_mode` rejects
  every invented mode with a usage error listing all six.

## Mid-Turn Submits (queued prompts + instant slash answers)

While an agent turn is running (`agent_running`): informational slash
commands keep answering instantly (dispatch is pure transcript work);
`/usage` runs on its OWN channel (`usage_rx`, drained by
`drain_usage_event`) instead of hijacking the agent channel, so the quota
answer lands mid-turn; the slash-command palette stays visible; and a
free-text prompt or workflow submit is QUEUED (`queued_prompt`) and fired
by the main loop through the shared `spawn_submitted_prompt` path the
moment the turn completes — never silently dropped, never interrupting the
work (ACP mid-turn-slash-grace parity; see `apps/agent-vesper-acp/AGENTS.md`).

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
