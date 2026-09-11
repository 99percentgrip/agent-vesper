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

- `lens_tools` owns shared native artifact-review and planning-interview tools:
  bounded question validation, workspace confinement, real Lens invocation and
  provider-visible feedback serialization. Both hosts use the same executor;
  hosts own URL presentation and the live question limit. ACP emits a review URL
  through its event sink; TUI also attempts desktop-browser launch.
  `tests/swarm_lens_browser.mjs` is driven by the explicit native worker browser
  gate in `swarm_adapter_tests`: actual Chrome submission, returned tool status,
  captured provider continuation and retained history, with isolated state.

- `swarm_settings` owns bounded workspace preferences and session-local explicit
  Save/Cancel drafts; reads and cancellation create no state. `swarm_service`
  owns one admitted native goal, real configured embedding/backend checks,
  existing tool/permission/progress composition, retained partial results and
  verified cleanup. Caller drop signals cancellation while the owned task settles.
  Cleanup uncertainty closes further service admission; preference alone never
  grants tool/network access. Explicit sandbox disable aliases and backend
  selection are honored; missing compiled backends and unknown selections refuse.
  Namespace probing runs off the async executor. Both hosts compose this behind
  default-off `swarm`.
- `swarm_inputs` captures bounded project files (4 MiB each, 64 MiB aggregate,
  4096 files, 64 levels) and materializes separate worker copies. Known project
  dotfiles (`.github`, `.cargo`, `.gitignore`, `.gitattributes`, `.editorconfig`,
  `.dockerignore`) are included; other hidden state and generated dependency/build
  directories are excluded, symlinks/special files and
  excessive inventories refuse. Source project files are never worker write roots.
  Explicit-run directories remain as output artifacts.
- `swarm_embedding` bridges a host's real configured embedding implementation on
  bounded blocking tasks; unfinished requests retain permits. Setup/recall share a
  process-wide four-request bound, including abandoned observers. No hash fallback.
  `swarm_journal` retains native worker/task identities and full returned histories
  in memory through observer cancellation, with explicit settlement observation.


- `src/swarm_adapter.rs` (feature `swarm`, default-off) adapts `WorkerPort`
  through the existing `AgentLoop`, not a second provider-stream loop. Hosts
  inject `WorkerFactory` (registry/config/credentials), real `ToolRegistry`,
  mode/permission, approval and optional progress ports. Role/task restrictions
  remove executable registrations and prefix gateways, not only schemas.
  Each instance refuses concurrent reuse and reports pending owned work until
  its native loop settles, even after observer drop; each task creates its own native
  runtime session. `into_instance_factory` produces the shared native pool
  factory: registry/config/permission/progress services are inherited, but busy
  flags and histories are independent. Session startup remains lazy inside the
  existing AgentLoop, not a second boot-time provider call.
  Completed/interrupted histories remain in memory, never
  implicitly persisted. Incomplete/cancelled outcomes cannot become success.
  `tests/swarm_adapter_tests.rs` uses a scripted provider and real read/write
  executors to check continuation, denial, restriction, cancellation, bounds
  and interrupted-history preservation with temporary workspace roots.
  Native Settings and TUI/ACP orchestration composition are wired behind the
  opt-in build; full cross-host acceptance remains in the repair matrix. ACP has
  `AcpEngineProgressPort`, so lack of progress support is not an exclusion.

- `src/swarm_sandbox.rs` (optional `swarm`) binds native worker factories to
  one shared `LeaseBook` and real `SandboxBackend`. Each role/instance receives a
  fresh canonical child directory, the same permission port, and a scoped native
  command route. Worker roots remain output artifacts; cleanup never deletes
  results. Existing/symlinked roots refuse before provisioning. Filesystem
  capability is mandatory; network access also requires full capability and
  explicit grant provenance. Explicit shared mode uses one existing Docker
  supervisor and disjoint `w/<worker-id>` roots. `swarm_shared_scope` invokes
  the bundled checksum-pinned setpriv utility with Landlock, dropped capabilities,
  no-new-privileges and unique non-root worker credentials. A real confinement
  probe is mandatory; the namespace backend retains its isolated one-run protocol.
  Each command verifies all processes with its worker UID have exited and restores
  the original bind-mount owner before native tool continuation (rootful Docker
  and rootless Podman mappings differ). Ownership records stay supervisor-private. The bundled init reaps
  orphans. Cleanup uncertainty quarantines the scope even if the supervisor is
  subsequently removed. No new Rust syscall or sandbox backend is introduced.
  Scope preparation owns cancellation cleanup even when its blocking observer
  drops. Native backend work runs outside book/metadata locks; uncertain provision
  or teardown retains quarantine. Detached command ports keep their leases.
  One-run backends rotate supervisors only after verified teardown while retaining
  the worker reservation across continuations. Dedicated roots explicitly permit
  private SELinux labels; ordinary project roots are never relabeled by this path.
  `settle`/`shutdown` and bounded error diagnostics expose actual cleanup outcomes;
  consuming backend teardown offers no assumed safe retry. The factory's
  `with_sandbox_leases` applies to startup, growth and replacement. Host commands
  and Settings remain acceptance-gated; the adapter alone is not product activation.
  `src/swarm_sandbox_tests.rs` verifies no-state refusals, unused preparation,
  uncertainty and symlink canary preservation. `tests/swarm_native_hive.rs` has
  explicit namespace/container gates for real 1+3-worker execution, two approved
  commands per worker, scoped permission traces, grounded synthesis, scale,
  retirement, replacement and verified shutdown. Unavailable capabilities fail
  these explicit gates instead of skipping their bodies.

- `src/sandbox_backend.rs` owns the shared native command adapter used by TUI
  and ACP. Current backends execute Linux payloads using absolute `/bin/sh`,
  including containers hosted on Windows/macOS. Nonzero command exit preserves
  stdout/stderr as failure; verified cleanup never converts it to success. Teardown failure overrides run success/cancellation, preserves available
  command output or the run error in diagnostics, and permanently quarantines that
  port against subsequent provisioning. Cancellation is rechecked after provision
  and after cleanup. Already-admitted concurrent operations are not retroactively
  cancelled. Unwinding backend panics during provision/run/teardown construction
  or polling become errors and permanently quarantine the route. A run panic still
  reaches explicit teardown. Panic payloads are excluded from tool diagnostics;
  the process panic hook is unchanged. Abort/double-panic, blocking hangs and
  process-tree verification remain acceptance gaps. No automatic cleanup retry or
  quarantine reset is exposed. `src/sandbox_outcome_tests.rs` checks outcome
  arbitration and quarantined refusal; `src/sandbox_panic_tests.rs` covers provision
  construction/poll panics, phase guards and ordinary refusal without quarantine.
- `tests/swarm_native_hive.rs` runs real Hive/native-factory/AgentLoop/read_file
  composition under all four topology configurations. Three independent provider
  sessions must overlap at a barrier; tool results feed continuation and synthesis,
  and temporary roots must remain free of implicit durable state. Provider and
  embedding fixtures are test-only. This is not supervisor or native host activation
  acceptance, nor proof that topology edges constrain all dispatch.

- `src/web_settings.rs` owns explicit workspace web-setting saves and shared
  `/web` controls. Atomic private JSON snapshots preserve existing TOML and
  its initial allowlist/budget values. Read/status/cancel never create state.
  Docker/Podman discovery honors the operator override, runs only in explicit
  settings/setup flows, has a five-second timeout, and normalizes bare Podman
  IDs to `sha256:`. Inspection spawn failures retain OS error kind/code without
  leaking executable paths or falsely diagnosing daemon availability.
  `setup_driver` reads only the executable-adjacent bundle
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

- ADR 0028: `acceptance`, `acceptance_snapshot`, `acceptance_runner` and
  `acceptance_settings` own original PRD/project-rule capture, independent read-only
  review, exact Rust test execution, private in-memory receipts and native controls.
  Snapshot bounds are 128 MiB/16384 files/64 levels; symlinks, special files and private
  .env inputs refuse. Default reads write nothing; settings saves/audit export are
  explicit. Resume imports history only, never verification. `acceptance_tests` covers
  real defective/repair execution and adversarial failures.
  Saved opt-in discovery treats unresolved or absent workspaces as inactive, so
  ordinary ACP sessions remain usable. Explicit enrollment/saves require an existing
  absolute workspace; unreadable or malformed existing settings still refuse.
  Verification forwards only the explicit toolchain environment allowlist, including
  Windows SDK/MSVC discovery roots; those same values bind the receipt environment.

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

- Driver CLI fixtures use the immutable executable `tests/container_cli_fixture.sh`
  through per-test symlinks with response/load state in temporary roots. Do not
  execute freshly written test scripts: concurrent fork/exec can retain a writable
  reference and trigger Linux ETXTBSY even when the writing thread closed its file.
  Never rewrite a just-executed inode or add production retries to mask this fixture
  race.
- Run `cargo test -p vesper-harness`.
- Run `cargo test -p vesper-harness --test vro13_e2e` (VRO-13 PR-8
  cross-feature fixture: watcher fire → bounded turn → composed
  firewall → opt-in sandbox route → scope-keyed slot ledger; run with
  `--features docker` to include the feature-gated cold-start arm).
- Run `cargo xtask architecture` and `cargo xtask verify`.

## Child DOX Index

No children.
