# vesper-harness — shared hosted agent services

## Purpose

Own the composition-neutral hosted tool service used by production frontends.
It supplies the Python-oracle memory, skills, awareness, deliberation,
failure-corpus, cron, session-search, delegation, source-inspection,
transactional patch, workflow, signed-plugin, worktree, and MCP gateway tools.
It also exposes the Python-compatible `web_search`, `web_reader`,
`vision_analyze`, and permission-gated `browser_ui` presets through protected
Z.ai and Playwright MCP server descriptors.

## Ownership
## Ownership

- `src/swarm_adapter.rs` (VRO-15 PR-9, feature `swarm`, **default-off**)
  owns the `WorkerPort` execution adapter: one swarm turn maps to one
  `ProviderSession::start` stream under a cancellation bridge, and the
  tool registry is filtered per task against the role class allowlist ∩
  task required capabilities. `vesper-provider` and `vesper-swarm` are
  optional deps linked **only** when `swarm` is explicitly enabled; the
  default build (single-agent loop, TUI, ACP) is byte-identical to
  pre-VRO-15 and links neither. Host parity decision (VRO-15 PR-9): the
  opt-in `/swarm` host surface is **initially TUI-only**; the ACP host
  gains it only when a protocol-level interaction model is defined
  (multi-agent progress is not expressible in ACP v1 session turns
  without host-owned polling). This exclusion is documented here and in
  `apps/agent-vesper-acp/AGENTS.md` per the root parity contract.

- `src/web_settings.rs` owns explicit workspace web-setting saves and shared
  `/web` controls. Atomic private JSON snapshots preserve existing TOML and
  its initial allowlist/budget values. Read/status/cancel never create state.
  Docker/Podman discovery honors the operator override, runs only in explicit
  settings/setup flows, has a five-second timeout, and normalizes bare Podman
  IDs to `sha256:`. `setup_driver` reads only the executable-adjacent bundle
  (or explicit `AGENT_VESPER_BUNDLE_DIR`), verifies SHA-256 before importing,
  caps import at 180 seconds, and verifies the exact bundled image ID after
  import. This explicit installer/settings operation is the exception to
  workspace-root I/O confinement; it is never a model-facing tool. No image
  download or container launch occurs. Both hosts expose `--setup-web-driver`
  before provider boot and `/web setup` for workspace selection. Web runtime
  CLI selection also finds Podman when Docker is absent. No sandbox or
  private-address protection may be switched off by these controls.
  An enabled runtime without an explicit image override uses the bundled
  immutable image ID; activation never requires copying a digest by hand.

- `src/lib.rs` owns the shared service, bounded durable-store wiring, and
  provider-worker delegation boundary.
- Frontends own provider selection, approval UI, ACP/TUI protocol mapping, and
  session lifecycle; they inject a `WorkerFactory` when nested work is allowed.

## Local Contracts

- Every filesystem path is confined to the caller's primary workspace root
  before access.
- `src/slash_commands.rs` owns store-backed slash-command execution for the
  ACP and TUI compositions: `execute_slash_command` resolves catalog commands
  against a host-supplied `SlashCommandContext` (durable `MemoryStores`,
  model/plan labels, mode), `/help` renders the oracle fixture text
  byte-exactly, `/curator` runs deterministic curation against the memory
  store, provider-facing switches validate into a `SessionOverrides` payload
  the host applies at its own provider boundary, and commands only a
  frontend can serve (conversation state, workflow turns, live provider
  quota) return `SlashCommandOutcome::Host` passthrough. The catalog and
  parser delegate to `vesper-domain::slash_commands`.
  `/max-iterations` accepts `enable`, `disable`, or `1-1000`; the optional
  user cap is disabled by default and never removes the agent loop's ultimate
  safety ceiling.
- `MemoryStores::parity_report` owns the shared read-only repository,
  meta-learning, observability, and journey renderings used by ACP and
  available to the TUI composition.
- `src/host_commands.rs` executes the store-backed host commands
  (`/checkpoint`, `/rollback`, `/undo`, `/export`, `/sessions`, `/lineage`,
  `/ci`, `/plugins`, `/mcp`) on `HarnessToolService` against the same
  durable checkpoint/MCP roots the TUI drains through, with byte-identical
  response formats, so the ACP composition reaches TUI parity without
  duplicating drain logic. `/checkpoint` and `/lineage` seed a session
  lineage record named for the host session id on first use.
- Checkpoints/lineage are OPT-IN per composition:
  `HarnessToolService::new_with_checkpoint_gate(..., checkpoints_enabled)`
  (the plain `new` keeps the historical enabled default for the TUI). A
  gated service never creates the checkpoint or lineage directories at
  construction, never spawns the cron scheduler, and answers the five
  checkpoint-family commands with the truthful
  `AGENT_VESPER_ENABLE_CHECKPOINTS` opt-in notice. The ACP host builds
  gated-by-default (root AGENTS.md contract); `/ci` and `/export` remain
  available either way.
- `MemoryStores::open_default` opens the project root
  (`AGENT_VESPER_MEMORY_ROOT` → `.agent-vesper/memory/`) plus the
  cross-project global skill layer (`AGENT_VESPER_GLOBAL_MEMORY_ROOT` →
  `~/.agent-vesper/memory/`); `open_at` is the explicit-root constructor
  compositions with their own root resolution share so the TUI and ACP can
  never drift on store-open semantics again.
- Durable roots are supplied by the composition boundary and default to the
  `.agent-vesper/` layout; no credentials are persisted by this crate.
- `src/scope_holder.rs` resolves the shared VRO workspace scope once at host
  boot. The TUI uses its default writing stamp policy; ACP must pass
  `StampPolicy::ReadOnly` unless `AGENT_VESPER_ENABLE_SCOPE_STAMP=1`, so an
  editor-spawned process never creates `.vesper-scope-id` in a project by
  default. The policy changes persistence only, never the resolved id.
- The service exposes the same hosted tool definitions and behavior to ACP and
  TUI, avoiding frontend-specific parity drift.
- `HarnessToolService::orchestrate_skills` is the shared ADR 0024 host bridge.
  It supplies current tool capabilities and platform to `vesper-memory`, owns
  bounded outcome feedback, and shares that tracker with read-only workers.
  It never turns selection into permission.
- Provider-backed worker outcome rendering preserves partial content and the
  typed interruption diagnostic from the shared agent loop; workers never
  silently report an interrupted generation as complete.
- Provider-backed workers render ultimate iteration-cap outcomes explicitly;
  unfinished native-plan work is never reported as completed.
- First-party MCP presets are not persisted and cannot be shadowed by custom
  registry entries. Web and reader calls use the configured Z.ai API-key
  environment; vision paths are workspace-confined; browser actions are an
  explicit allowlist and never arbitrary JavaScript evaluation.
- All output, source scans, batches, workflow depth, and worker actions remain
  bounded. When a provider-backed worker is supplied, the host starts a
  one-second polling scheduler that claims due cron jobs, executes them, and
  persists bounded status/output; dropping the service aborts that scheduler.
- Daemon locks and PID watchers share one bounded native liveness probe:
  `/proc/<pid>` on Linux, `kill -0` on other Unix hosts, and `tasklist` on
  Windows. If the probe cannot run it conservatively treats the PID as live,
  preventing accidental lock takeover or false watcher fires.
- No protocol, provider-wire, UI, SQLite, or live-provider dependency is
  allowed here.
- **Phase 3 deferred loading + MCP gateway.** `mcp_list_tools` now translates
  discovered MCP tool descriptors into `ToolDefinition`s named
  `mcp__<server>__<tool>` (with `defer_loading = false`) and returns them via
  `ToolResult::with_injected_tools` instead of a stringified text payload.
  `McpGatewayExecutor` (registered under the `mcp__` prefix by
  `HarnessToolService::build_default_registry`) parses the call name back into
  `(server, tool)` and dispatches to `McpClient::call_tool`. Workers and ACP
  composition now route through this gateway so injected MCP tools advertised
  on the next turn are actually executable when the model calls them by name.

## Work Guidance

- Add provider-neutral core tools to `vesper-agent`; add durable or host-bound
  tools here and inject them through `ToolRegistry::with_service`.
- Preserve the fail-closed permission gate in the agent loop; this service
  never bypasses it.

- `src/web_service.rs` (VRO-14 PR-5) hosts the five opt-in web tools
  (`web_fetch`, `web_scrape`, `web_map`, `web_crawl`, `web_interact`) as
  `ToolExecutionClass::Network` with `defer_loading = true`. Both hosts
  explicitly attach the boot scope through `with_web_scope`. The hosted
  service itself advertises and dispatches web tools so direct TUI wrappers,
  ACP registries, discovery, and worker services all reach the same executor.
  With no effective web scope (or
  `enabled = false`) zero web tools register and the registry path is
  byte-identical to the pre-web build. The process-global web holder
  (`web_service::holder`) mirrors the firewall/sandbox holders.
- Passive web tools execute through the shared service's `FetchTransport`;
  production uses the sandbox helper on a blocking worker, tests use offline
  pages. Registry builds reuse the service Arc. `search_tools` searches eligible
  web definitions and injects their schemas for subsequent model requests.
- `src/web_runtime.rs` lazily owns a single sandbox route used by fetch,
  ephemeral rendering, and persistent browser interaction. Initialization
  requires a digest-pinned driver image; no host HTTP/browser fallback exists.
  `AGENT_VESPER_SANDBOX=off` refuses initialization, including web execution.
  Browser interaction has a separate opt-in. A failed pipe invalidates the
  session, and render escalation never modifies the interactive session.
  Four shared runtime permits cap concurrent sandbox operations. Fetch and
  render share one deadline; crawl includes seed work in its wall-clock budget.
  Argument validation precedes external execution; closing an unopened browser
  is an idempotent no-op without a daemon/image probe.
- Map merges bounded sitemap/index discovery with page links. Current
  acceptance status remains in `docs/foundation/vro14-gap-audit.md`; a passing
  component test alone is not permission to advertise full PRD completion.

## Verification

- Driver CLI fixtures use a fresh immutable executable per case; tests must
  not rewrite a just-executed inode while exercising process inspection.
- Run `cargo test -p vesper-harness`.
- Run `cargo test -p vesper-harness --test vro13_e2e` (VRO-13 PR-8
  cross-feature fixture: watcher fire → bounded turn → composed
  firewall → opt-in sandbox route → scope-keyed slot ledger; run with
  `--features docker` to include the feature-gated cold-start arm).
- Run `cargo xtask architecture` and `cargo xtask verify`.

## Child DOX Index

No children.
