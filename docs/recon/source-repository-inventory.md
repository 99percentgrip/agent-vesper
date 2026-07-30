# Source Repository Inventory

Status: COMPLETE

## Scope and evidence policy

This report inventories the frozen reference repository at `/home/alex/Projects/Native GLM-5.2 Provider`. “Confirmed” means the implementation or automation was inspected. README-only claims are not treated as confirmed unless paired with code or tests. Proposed Rust placement belongs in the module map and architecture reports.

## Repository state

| Attribute | Target: Agent Vesper | Source: Native GLM ACP |
|---|---|---|
| Absolute path | `/home/alex/Projects/Agent Vesper` | `/home/alex/Projects/Native GLM-5.2 Provider` |
| Repository root | No Git repository at first inspection | Same as absolute path |
| Remote | None | `origin = https://github.com/99percentgrip/Native-GLM-ACP.git` |
| Branch / commit | Not applicable | `agent/jit-tool-loading` / `bf4d4287e2e3320aa3f09015f678e6169d520045` (`Finalize v2.7.34 release documentation`) |
| Working tree | Initially only `AGENTS.md` | One pre-existing untracked `docs/codex-tui-roadmap-prompt.md`; ignored caches, screenshots, local quality output, and PyInstaller specs also exist |
| Primary language | Documentation-only at inspection | Python 3.10+ |
| Build/package | None | Hatchling, `uv.lock`, wheel/sdist and PyInstaller frozen distributions (`pyproject.toml:1-72`) |
| Version | None | `2.7.34` (`registry/agent.json:4`) |

**Identity verdict — confirmed.** The directory is the expected completed ACP harness: its Git remote names Native-GLM-ACP; `pyproject.toml` declares `glm-acp`; `glm_acp/agent.py:674` implements `GlmAcpAgent(acp.Agent)`; `glm_acp/glm_client.py:111` implements the Z.ai streaming client; and the executable routes through `glm_acp.cli:main` (`pyproject.toml:52`, `glm_acp/__main__.py:3-6`). Planning may proceed.

## Size and concentration

Tracked Python production and tests total 44,487 lines. Production is approximately 28,000 lines and tests approximately 16,500 lines. The dominant modules are:

- `glm_acp/agent.py` — 7,235 lines: ACP adapter, session aggregate, agent loop, permission orchestration, commands, compaction, workers, and many feature coordinators.
- `glm_acp/tui.py` — 5,016 lines: Textual frontend, modal screens, reducers, terminal integrations, and export/voice/mobile/worktree UX.
- `glm_acp/tools.py` — 2,082 lines: tool schemas, dispatcher, filesystem/search/command operations, memory and extension tool adapters.
- `glm_acp/memory.py` — 1,222 lines: project/user memory, learned skills, bundles, curation, and evaluation-gated evolution.
- `glm_acp/checkpoints.py` — 843 lines and `glm_acp/glm_client.py` — 804 lines.

This is a production system, not a narrow provider shim. The size concentration is itself migration evidence: `agent.py`, `tui.py`, and `tools.py` must be decomposed by behavior rather than translated as units.

## Entrypoints and operating modes

| Surface | Entrypoint | Confirmed behavior | Test anchors |
|---|---|---|---|
| ACP stdio | `glm_acp/cli.py:main` → lazy `glm_acp.agent:run` at `cli.py:181`; `agent.py:7222` | Bare command starts ACP SDK stdio server; agent implements ACP lifecycle | `tests/test_quality.py` process lifecycle tests; `tests/test_agent.py` lifecycle tests |
| Python module | `glm_acp/__main__.py:3-6` | Routes to the same CLI | `tests/test_cli.py` |
| Frozen binary | `glm_acp/launcher.py:3-6` | Absolute import of same CLI for PyInstaller | CI and release workflows |
| Plain/JSON terminal | `glm_acp/terminal_cli.py:410 run_chat`, `:534 run_chat_command` | Builds the same agent and routes commands/config through shared APIs | `tests/test_terminal_cli.py` |
| Full-screen TUI | `glm_acp/tui.py:2047 NativeGlmTui`, `:4992 run_tui_command` | Textual frontend over the shared agent/session update interfaces | `tests/test_tui.py` |
| Cron CLI/daemon | `glm_acp/cron_cli.py`, `glm_acp/cron_scheduler.py:317 daemon` | Persistent scheduler and explicit daemon/tick commands | `tests/test_cron.py` |
| Plugin CLI | `glm_acp/plugin_cli.py:27` | Package signing, verification, trust, and install management | `tests/test_plugin_runtime.py`, `tests/test_safety_roadmap.py`, `tests/test_hardening_roadmap.py` |

## Build, CI, release, and platforms

- CI runs the complete test suite on Python 3.10, 3.11, 3.12, and 3.13 (`.github/workflows/ci.yml`, `python-compatibility`).
- The verify job checks selected formatting, shell syntax, fatal Ruff rules, benchmark catalog validity, tests, `pip-audit`, wheel/sdist creation, and isolated wheel execution (`.github/workflows/ci.yml`, `verify`).
- Platform jobs test and build frozen executables for Linux x86-64 and ARM64, macOS Intel and Apple Silicon, and Windows x86-64 (`.github/workflows/ci.yml`, `platform`).
- Tagged release jobs run tests, bundle optional local Whisper except on macOS Intel, build a PyInstaller onedir archive, verify version/TUI/assets, enforce a 200 MiB compressed ceiling, publish SHA-256 sidecars and provenance attestations, and release Python distributions plus Registry/install assets (`.github/workflows/release.yml`).
- Registry metadata is version-pinned across the same five targets (`registry/agent.json:1-43`). Unix and Windows installers verify SHA-256 before replacing user-local bundles (`scripts/install.sh`, `scripts/install.ps1`).
- Live quality evaluation is opt-in/manual and requires `ZAI_API_KEY`; normal CI validates fixtures without model spend (`.github/workflows/quality.yml`).

## Runtime and ACP protocol inventory

### Session aggregate

`glm_acp/agent.py:377 Session` owns workspace roots, messages, provider/model/reasoning/generation settings, permission/session modes, plan/goal state, usage, compaction data, lineage, tool-schema state, awareness/deliberation/repository intelligence, and per-turn locks/tasks. It serializes through `Session.to_dict` (`:555`) and validates/defaults old data through `Session.from_dict` (`:603`).

### ACP lifecycle — confirmed

- `initialize` starts deferred custom-MCP discovery without blocking startup and advertises protocol capabilities/auth (`agent.py:1842-1902`).
- `authenticate` accepts only the advertised terminal-auth method and requires configured credentials (`agent.py:1904`).
- `new_session`, `load_session`, `list_sessions`, `resume_session`, `close_session`, and `fork_session` are distinct operations (`agent.py:1909`, `:1941`, `:2008`, `:2031`, `:2088`, `:2104`). Lineage is persisted via `parent_session_id` and `branch_root_id` (`session_store.py:214-228`).
- Config changes serialize through the per-session prompt lock (`agent.py:2182-2324`); session mode changes use the same lock (`:2325-2355`).
- Prompts serialize through `prompt`/`_prompt_locked` (`agent.py:2356-2539`); `cancel` cancels the active provider and prompt task (`:2540-2550`).
- Streaming output is translated to ACP thought, message, tool-start/update/complete/failure, plan, usage, and session-info updates by helpers at `agent.py:4748`, `:6759-6950`.
- History replay is explicit (`agent.py:6790`) and therefore event order—not only stored content—is a compatibility surface.

**Ambiguous pending inspection:** exact `InitializeResponse` capability fields and every ACP update ordering branch still require a focused event trace; the behavioral-contract report remains in progress until that trace is complete.

## Provider implementation inventory

### GLM configuration

- Z.ai endpoints are Coding Plan, Standard API, and BigModel CN (`config.py:433-453`).
- Models and plan eligibility are a static registry (`config.py:548-633`); context windows range from 64K to 1M (`:495-503`).
- Thinking levels are Off, Standard, and GLM-5.2-only High/Max (`config.py:591-628`).
- Generation profiles apply either provider defaults, temperature `0.7`, or `top_p 0.98` (`config.py:455-475`).
- Default HTTP timeout is 180 seconds, output default 128K, automatic continuations cap at 20, retry count at 3 (`config.py:11-15`, `:415-419`).

### Authentication and endpoint safety

`GlmClient.__init__` reads the key through configuration, sets Bearer auth, and creates an `httpx.AsyncClient` (`glm_client.py:114-152`). Provider quota credentials may go only to an HTTPS allowlist of official Z.ai/BigModel hosts; custom API endpoints get no usage origin (`:136-147`). Credentials are environment-first and otherwise stored privately (`config.py:685-775`, tests in `tests/test_config.py`).

### Request and streaming contract

- `_do_stream_request` posts `/chat/completions` with model, normalized messages, streaming, max tokens, and usage request; conditionally adds thinking, `reasoning_effort`, tools/tool streaming, temperature, and top-p (`glm_client.py:519-555`).
- SSE parsing ignores non-`data:` lines and malformed JSON; recognizes `[DONE]`; separates `reasoning_content` from `content`; accumulates tool names/argument fragments by index; and normalizes prompt/completion/cache usage (`glm_client.py:641-801`).
- Reasoning is flushed before content within each coalesced batch, and pending text is flushed before tool-start callbacks (`glm_client.py:665-678`, `:744-762`).
- Cancellation marks the client and cancels the active request task (`glm_client.py:154-163`); the stream loop also checks cancellation between lines (`:691-695`).
- A stream ending without `[DONE]` or finish reason raises `IncompleteStreamError` (`:783-784`). If visible content/reasoning exists, retry is suppressed and `network_error` preserves partial output; otherwise retry proceeds (`:557-601`).
- Retryable HTTP statuses are 429/500/502/503/504; `Retry-After` accepts seconds or an HTTP date, capped at 60 seconds, otherwise jittered exponential backoff (`config.py:415-419`, `glm_client.py:623-639`).
- Finish reason `length` without tool calls produces an exact continuation prompt and preserves reasoning when required; it caps at 20 and returns `continuation_limit` rather than pretending success (`glm_client.py:169-230`).

Tests are unusually strong here: `tests/test_stream_integration.py` has dedicated content, reasoning, tool-call, usage, malformed stream, cancellation, retry, and continuation groups; `tests/test_glm_client.py` covers client-level request and quota behavior.

## Agent loop and context

`GlmAcpAgent._run_turn` (`agent.py:2836-3992`) is the shared loop. Confirmed surrounding controls include:

- System prompt construction and project/learned context (`agent.py:282 build_system_prompt`; `Session.refresh_system_prompt` at `:449`; `project_context.py:65-135`; `memory.py:197`).
- Per-turn maximum iteration bounds, repeated batch signature/argument validation (`config.py:15`, `:22`, `agent.py:2551-2572`) and result-aware guardrails (`guardrails.py:17 ToolLoopGuard`).
- JIT schema discovery starts a session with a gateway and loads selected native/MCP schemas in stable order (`jit_tools.py`; `agent.py:2573-2608`; `tests/test_jit_tools.py`).
- Read/search operations in one batch may run concurrently, while workflow execution is sequential (`tools.py:1730-1804`).
- Post-tool processing records changed paths, diagnostics, verification freshness, awareness evidence, repository impact, hooks, telemetry, checkpoint post-state, and failure data (`agent.py:2623-2762`).
- Plans and epistemic/deliberation updates have dedicated handlers (`agent.py:4618-4747`).
- Completion can be gated by persistent goals, fresh evidence, contradictions, and auxiliary judgment (`agent.py:1408`; awareness implementation/tests).
- Compaction starts at 85%, keeps four recent messages, preserves deterministic evidence categories, inserts a bounded provider summary transactionally, emits pressure tiers at 60/75/85%, and tracks quality decline (`config.py:483-541`; `agent.py:6300-6627`; `tests/test_compaction.py`).
- Delegation is permission-gated and bounded: three workers, six iterations each, 180 seconds, and shared 24-call/120K-input/16K-output budgets (`config.py:23-28`; `agent.py:4102-4617`; tests in `test_agent.py` and `test_reliability.py`).

## Native tool inventory

The schema list is declared in `glm_acp/tools.py:205-1117`, with the cron gateway at `:50-203`. The central dispatcher is `execute_tool` (`:1195-1400`). Permission classification is centralized in `config.py:636-667`, then combined with session mode, policy, hook decisions, and smart approvals in `agent.py:4772-5093`.

| Tool(s) | Behavior / permission class | Key invariants and evidence |
|---|---|---|
| `read_file` | Read-only | Strict UTF-8/binary detection, line windows and bounded output (`tools.py:1129`, `:1483`) |
| `write_file`, `edit_file`, `apply_patch` | Destructive | Workspace containment; text validation; write/edit semantics (`tools.py:1534-1656`) |
| `apply_patch_set` | Destructive | Requires SHA-256 preconditions, pre-commit syntax checks, prepares every candidate first, and rolls committed files back on write error (`tools.py:1657-1727`) |
| `list_directory`, `search_files`, `grep` | Read-only | Resolved roots, bounded results; `rg` fast path and ignored-directory fallback (`tools.py:1868-1972`) |
| `batch_read` | Read-only | Up to 20 allowed read/search operations concurrently, reduced bounded JSON (`tools.py:1730-1767`) |
| `run_command` | Destructive | Scrubbed environment, workspace cwd, optional OS sandbox, streamed head/tail output, timeout, full process-group kill on POSIX, Job Object containment on Windows (`tools.py:127-135`, `:1975-2082`) |
| `update_plan`, `update_awareness`, `update_deliberation` | Agent-state tools | Validated by agent handlers; do not bypass completion/permission policy (`tools.py:360-507`; `agent.py:4618-4747`) |
| memory/profile tools | Mixed; writes/destruction are gated | Project memory stays `.glm-acp/memory.md`; approved user preference is private config state; secret/promptware and containment checks precede writes (`memory.py:24-25`, `:286-495`) |
| skill/bundle/evolution tools | Mixed; mutations gated | Project-owned `SKILL.md`, usage/bundle/candidate metadata, explicit evaluation and promotion (`memory.py:496-1222`) |
| `semantic_code` | Read-only | Optional LSP manager; position conversion and bounded semantic responses (`diagnostics.py:65`, tests `test_extensions.py`) |
| `delegate_task` | Destructive/permission-gated | Read-only worker surface and shared budgets (`agent.py:4208-4387`) |
| `cronjob` | Destructive/permission-gated | CRUD/run gateway with scheduler recursion guard and contained workdirs/scripts (`tools.py:1403-1480`) |
| `run_workflow` | Destructive/permission-gated | Validated acyclic bounded DAG, sequential stop-on-failure (`workflows.py:21`; `tools.py:1770-1804`) |
| `plugin_package` | Destructive/permission-gated | List/verify/install/trust operations via data-only registry (`tools.py:1807-1838`) |
| `worktree_worker` | Destructive/permission-gated | Detached worktree creation, digest review, verification and transactional promotion (`worktrees.py`, `agent.py:4388-4617`) |
| `failure_corpus` | Destructive/permission-gated | Metadata-only draft management and explicit project-local test promotion (`tools.py:1841-1865`) |
| MCP `web_search`, `web_reader` | Read-like but remote | Stable first-party routes; remote output remains untrusted (`mcp.py:67-112`) |
| MCP `vision_analyze`, `browser_ui`, `mcp_list_tools`, `mcp_call` | Permission-gated | Browser is allowlisted/isolated; generic MCP and vision are destructive class because they cross external trust/side-effect boundaries (`mcp.py:67-164`; `config.py:657-660`) |

**Tool schema note:** Every schema property and error string is too large to reproduce here. Exact-output parity should freeze the source schemas as canonical JSON fixtures rather than manually recode prose.

## Security and isolation — initial confirmed inventory

- `Sandbox.resolve` canonicalizes roots and candidates and accepts only paths relative to an allowed root; symlink targets escaping a root are rejected by resolution (`tools.py:1165-1192`).
- Child commands exclude common API-key/token/secret/password/private/access-key/credential suffixes and `SSH_AUTH_SOCK` (`tools.py:127-135`). Hooks apply equivalent scrubbing (`hooks.py:14-38`).
- POSIX commands start a new session and timeout kills the process group; Windows creates a process group and optionally attaches a kill-on-close Job Object (`tools.py:1981-1984`, `:2052-2070`; `os_sandbox.py:138-209`).
- Sandbox modes are `off`, `auto`, and `required`; invalid values become `required`. Linux uses Bubblewrap when present, macOS uses detected Seatbelt, Windows explicitly refuses required filesystem/network isolation because Job Objects cannot provide it (`os_sandbox.py:16-27`, `:68-135`).
- Stored trusted context is blocked on promptware patterns; tool/retrieval output is bounded, annotated, and enclosed in `<untrusted_context>` (`security.py:22-112`).
- Hooks are workspace-scoped argv arrays whose executable bytes must match a SHA-256 pin; payload/stdout/time are bounded and failures isolated (`hooks.py:41-107`).
- Plugins require schema 1, data-only allowlisted extensions, manifest hashes, atomic install/rollback, optional Ed25519 publisher signatures, and a private trust store (`plugins.py:33-425`; tests `test_safety_roadmap.py`, `test_hardening_roadmap.py`).
- Checkpoints exclude common credentials/private keys and dependencies/build output, cap count/bytes/file size, store compressed content-addressed blobs outside workspace Git, and refuse rollback on later hash conflicts (`checkpoints.py:1-72`, `:142-850`).

The dedicated `security-invariants.md` remains in progress pending mobile/browser/MCP and permission-bypass traces.

## Persistence inventory — initial

| Store | Location and format | Writer/reader, compatibility, corruption |
|---|---|---|
| Sessions | Default `~/.glm-acp/sessions/<sanitized-id>.json`; profile path under `~/.glm-acp/profiles/<profile>/sessions`; `.meta` sidecars; `session-index.sqlite3` WAL/FTS5 | `SessionStore` (`session_store.py:56-413`); JSON writes and sidecars are atomic and 0600; malformed JSON loads as absent; legacy JSON is backfilled into FTS; index failures fail soft |
| Session search | `indexed_sessions` table plus FTS5 `messages_fts` | System messages omitted; bodies capped at 32K and credential patterns redacted (`session_store.py:83-164`); index is rebuildable, not authoritative |
| Credentials | Profile config `credentials.json` | Environment keys precede file; atomic 0600 storage (`config.py:685-775`) |
| UI/preferences | `max-iterations.json`, `statusline.json`, `theme.json`, `screen-reader.json`, `vim.json`, `keybinds.json` | Schema/versioned JSON, atomic private writes, malformed values fall back (`config.py:48-395`) |
| Project memory | `<workspace>/.glm-acp/memory.md` | Atomic workspace write, promptware/secret validation and exact forget (`memory.py:286-409`) |
| User profile | profile config user-memory file | Private atomic write, only explicit approved preferences (`memory.py:423-495`) |
| Learned skills | `<workspace>/.glm-acp/skills/<slug>/SKILL.md`; `.usage.json`; `.bundles.json`; archive/candidate JSON | Version 1 metadata; safe-path checks; explicit archive/restore/evaluation/promotion (`memory.py:496-1222`) |
| MCP configuration | profile config `mcp.json` or explicit env override | JSON read/write (`mcp.py:169-229`); atomicity/permissions need redesign because current direct `write_text` is weaker than other stores |
| Telemetry | profile config `trajectory.jsonl`, schema 1 | Append-only 0600, metadata allowlist (`telemetry.py:22-97`); malformed lines ignored by observability (`observability.py:33-57`) |
| Failure corpus | profile config `failure-corpus/drafts.jsonl`, schema 1 | Append/private; project identity hashed; explicit promotion writes `.glm-acp/evaluation/failure-cases.json` (`failure_corpus.py:43-210`) |
| Checkpoints | profile config `checkpoints/store/objects` and `checkpoints/workspaces/<hash>/*.json`; schema 2 manifests; schema 1 legacy directories | Atomic private objects/manifests, lock directory, verified legacy migration, bounded GC (`checkpoints.py:142-850`) |
| Cron | profile config `cron/jobs.json` version 1, lock, `results/<job>/<timestamp-token>.json`, daemon heartbeat | Cross-process lock, atomic fsync/replace, 0600; malformed/unsupported store fails closed (`cron.py:44-147`, `:487-643`) |
| Plugins/trust | profile config `plugins/<id>/...`, `trusted-plugin-publishers.json`; schema 1 | Atomic install with backup rollback; private trust and package files (`plugins.py:134-425`) |
| Hooks | profile config `hooks.json` | User-authored read-only config; invalid entries ignored (`hooks.py:27-77`) |
| Metacognition/deliberation/repository intelligence/meta-learning/awareness | Primarily embedded in session JSON; aggregate capability profile reads redacted trajectory | Session serialization is the compatibility boundary (`agent.py:555-673`) |
| Worker records | Private worker transcript path and worktree registry under profile config | `agent.py:4102-4135`; `worktrees.py:21`; exact lifecycle pending persistence report |

## TUI and UX inventory — initial

`NativeGlmTui` (`tui.py:2047-4991`) owns presentation; `TuiClient` (`:1833-1886`) adapts ACP-style updates/permissions. Runtime data comes from the shared `GlmAcpAgent`, `SessionStore`, memory, plugins, mobile server, provider usage, voice, terminal-image, and Git/worktree helpers.

Confirmed screens include permission, settings, session history, conversation search, learning journey, code-block picker, configurable status line, keybindings, context budget, ask-while-working overlay, tasks/queue, worktree creation, plugins, and diff annotation (`tui.py:581-2046`). The app supports:

- conversation/reasoning/tool/plan/session/quota/activity display;
- prompt queue, cancellation, slash catalog and Ctrl-P command palette;
- settings routed through shared session APIs (`tui.py:2698`);
- persistent sessions/history/search/undo;
- working-tree changes/Git/diff/files/GitHub views (`:3959-4573`);
- worktree session tabs, mobile approvals, image attach/render/screenshot;
- local Whisper voice, optional sounds and desktop notifications;
- system clipboard helpers with scrubbed environments/bounds (`:239-359`);
- screen-reader, theme, Vim input, configurable bindings/status line;
- Markdown/JSON/clipboard/file exports (`:4829-4950`);
- bounded shutdown (`:3405-3440`, `:4986`).

`tests/test_tui.py` is 3,017 lines and directly tests most presentation contracts. Mouse details, every binding, and reducer event-order coverage will be enumerated in the parity strategy.

## Testing maturity

The suite is broad and implementation-facing:

- Protocol/agent: `test_agent.py`, `test_quality.py`, `test_terminal_cli.py`.
- Provider/SSE: `test_glm_client.py`, `test_stream_integration.py`.
- Tools/security: `test_tools.py`, `test_extensions.py`, `test_security.py`, `test_safety_roadmap.py`, `test_hardening_roadmap.py`.
- Persistence: `test_session_store.py`, `test_memory.py`, `test_cron.py`, checkpoint/plugin sections of roadmap tests.
- Context/reliability/learning: `test_compaction.py`, `test_reliability.py`, `test_awareness.py`, `test_metacognition.py`, `test_deliberation.py`, `test_repository_intelligence.py`.
- UI/platform/packaging: `test_tui.py`, `test_voice.py`, `test_terminal_image.py`, `test_mobile_server.py`, `test_installers.py`, `test_release.py`, `test_registry_package.py`, `test_uninstall.py`.

No dedicated Rust-independent wire fixture corpus, model-based state-machine suite, or fuzz target is tracked. Python “fuzzing” is an offline deterministic hardening command rather than a coverage-guided fuzzer (`resilience.py`). Cross-platform CI is strong but real sandbox backend behavior is capability-dependent and often asserted structurally/mocked.

## Significant generated, ignored, or local content

The source contains existing `__pycache__`, `.pytest_cache`, `.ruff_cache`, screenshots under `.glm-acp-images`, local `quality/` benchmark outputs, and PyInstaller `.spec` files. They are not tracked (except `.glm-acp/skills` metadata) and are excluded from source-size counts. No vendored dependency tree is tracked; `uv.lock` is the dependency lock.

## Unresolved questions

1. Exact ACP capability payloads and update order across new/load/resume/fork/cancel.
2. Complete session JSON field-by-field compatibility matrix and schema evolution (sessions have no explicit top-level schema version).
3. Exact permission precedence among Read Only, policy, hooks, smart approval, mobile approval, and Bypass.
4. MCP reconnection/session-expiry semantics and browser isolation details.
5. Full TUI command/keybinding/mouse matrix and state-source mapping.
6. Full-suite baseline: collection succeeds with 879 tests. An isolated full run passed its first 24 tests and then stopped progressing at `tests/test_agent.py::TestConfigSwitch::test_auxiliary_model_switch_and_plan_fallback`; it was interrupted without a failure traceback. This remains an unresolved baseline issue.

## Completion status

Repository identity, size, build/release/platform state, primary runtime, provider, agent, tools, persistence, security, TUI, and test surfaces are inventoried. Focused traces are incorporated in the behavioral/security/persistence reports. The incomplete full test baseline is explicitly recorded and carried as a blocker rather than preventing this inventory from being independently complete.
