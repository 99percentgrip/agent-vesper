# Python-to-Rust Module Map

Status: COMPLETE

## Scope and classification

This is a responsibility map, not a translation manifest. Categories are: **Core**, **GLM**, **ACP**, **Frontend**, **Tools**, **Persistence**, **Policy**, **Security**, **Memory**, **MCP**, **Workers**, **Automation**, **Observability**, **Platform**, and **Legacy**. Evidence paths refer to the frozen source repository.

## Runtime, provider, transport, and frontend

| Python path / evidence | Actual responsibility and important callers | Category | Proposed Rust location | Disposition and parity risk |
|---|---|---|---|---|
| `glm_acp/__init__.py` | Version constant consumed by ACP/MCP/CLI/release | Core | workspace package metadata | Merge into build metadata; low risk |
| `__main__.py:3-6`, `launcher.py:3-6` | Python/frozen entry shims to `cli.main` | Frontend/Legacy | `vesper-cli/src/main.rs` | Retire shims after packaging parity; alias risk |
| `cli.py:40 build_parser`, `:89 main` | Top-level lazy command routing, setup/uninstall/observe/hardening/meta commands | Frontend | `vesper-cli::{args,commands}` | Redesign typed subcommands; exact exit/help/startup risk |
| `agent.py:377 Session`, `:674 GlmAcpAgent`, `:2836 _run_turn` | Session aggregate, ACP adapter, shared loop, commands, permissions, feature coordination | Core/ACP | split among `vesper-core`, `vesper-acp`, `vesper-command`, facade in `vesper-runtime` | Must split; highest hidden-coupling/event-order risk |
| `glm_client.py:111 GlmClient` | Z.ai auth/request/SSE/usage/retry/continuation/quota | GLM | `vesper-provider-glm::{client,sse,quota}` over `vesper-provider` | Redesign behind provider port; highest provider parity risk |
| `config.py:436 API_ENDPOINTS`, `:548 MODELS`, `:591 THOUGHT_LEVELS` | GLM registry plus shared iteration/UI/config persistence | GLM/Core/Persistence | split `vesper-provider-glm::catalog`, `vesper-config` | Split; avoid provider leakage |
| `terminal_cli.py:60 TerminalClient`, `:410 run_chat` | Plain/JSON frontend and shared-session config routing | Frontend | `vesper-cli::chat` + `vesper-frontend::ClientSink` | Port semantics; output fixture risk |
| `tui.py:2047 NativeGlmTui`, `:1833 TuiClient` | Textual screens, reducers, terminal subprocess integrations | Frontend | `vesper-tui::{app,state,update,views,terminal}` | Redesign reducer-first; very high UX risk |
| `terminal_image.py:23` | Kitty/iTerm/link detection/rendering | Frontend/Platform | `vesper-tui::media` optional | Port, platform-terminal risk |
| `voice.py:89 VoiceRecorder`, `:153 transcribe_audio` | Local recorder/Whisper, sounds, notifications | Frontend/Platform | `vesper-tui::voice` optional feature | Redesign behind ports; packaging risk |
| `mobile_server.py:51 MobileServer` + `_mobile_pwa/*` | Pairing/approval HTTP server and static PWA | Frontend/Security | `vesper-approval-mobile` optional | Split from TUI; auth/replay/bind risk |
| `uninstall.py` | Frozen-install guards, profile/PATH/Zed cleanup | Legacy/Platform | installer project/release scripts | Redesign per Rust install layout; do not port self-delete assumptions |
| `release.py` | Version/tag/CI/release helper commands | Automation | `xtask` or CI scripts, not runtime | Retire runtime coupling; release parity risk |

## Tools, policy, security, and platform

| Python path / evidence | Responsibility/callers | Category | Proposed Rust location | Disposition/risk |
|---|---|---|---|---|
| `tools.py:205 TOOL_DEFINITIONS`, `:1195 execute_tool` | Schemas, dispatcher, filesystem/search/command/memory adapters | Tools | `vesper-tools::{catalog,fs,search,process,batch}` | Split schemas from executors; high security/schema risk |
| `policy.py:12 PolicyEngine` | Ordered workspace allow/ask/deny rules | Policy | `vesper-policy::{document,evaluator}` | Port fail-closed semantics; regex/path risk |
| `security.py:76 scan_promptware`, `:98 wrap_untrusted_output` | Stored-context blocking and untrusted delimiters | Security | `vesper-security::promptware` | Port plus adversarial fixtures; heuristic drift risk |
| `os_sandbox.py:68 command_prefix`, `:138 WindowsJob` | Bubblewrap/Seatbelt/Job Object capability truth | Security/Platform | `vesper-security::sandbox::{linux,macos,windows}` | Redesign native typed backends; very high platform risk |
| `hooks.py:41 LifecycleHooks` | Hash-pinned workspace-scoped lifecycle processes | Security/Tools | `vesper-tools::hooks` using security/process ports | Port; timeout/process/authority risk |
| `diagnostics.py:65 _LspProcess`, `:265 DiagnosticsManager` | Syntax validation and optional LSP JSON-RPC | Tools/Platform | `vesper-diagnostics::{syntax,lsp}` | Split; language-server lifecycle risk |
| `references.py:296 expand_references` | Bounded contained @file/folder/symbol/diff expansion/ranking | Core/Security | `vesper-context::references` | Redesign using repository index; ranking parity semantic |
| `jit_tools.py` (`ToolRegistry`, search gateway) | BM25/safe-regex schema discovery and collision-safe MCP routes | Core/Tools | `vesper-tools::registry` | Port stable order/limits exactly; cache-prefix risk |
| `workflows.py:21 ordered_steps` | Validates bounded acyclic static DAG | Tools/Policy | `vesper-tools::workflow` | Port; policy closure required |
| `resilience.py` | Offline parser fuzz/fault-injection command | Security/Testing | test/fuzz workspace, not production crate | Redesign as cargo-fuzz/proptest suites |

## Persistence, context, memory, learning, and observability

| Python path / evidence | Responsibility/callers | Category | Proposed Rust location | Disposition/risk |
|---|---|---|---|---|
| `session_store.py:56 SessionStore` | JSON sessions/meta plus rebuildable SQLite FTS5 | Persistence | `vesper-sessions::{model,json_compat,index}` | Preserve read compatibility; migrate writes later; corruption risk |
| `profiles.py:13 active_profile`, `:21 profile_path` | Validated profile isolation | Persistence/Security | `vesper-config::profile` | Port before any store; traversal risk |
| `project_context.py:47 project_root`, `:65 instruction_files`, `:137 ProjectFacts` | Project root/rules/manifests/VCS/check discovery | Core | `vesper-context::project` | Port semantics, redesign caching |
| `verification.py:80 classify_verification`, `:126 VerificationLedger` | Fresh edit-generation evidence and canonical checks | Core | `vesper-core::verification` | Port as typed state machine; completion risk |
| `guardrails.py:17 ToolLoopGuard` | Repeated failure/unchanged-read recovery | Core | `vesper-core::loop_guard` | Port exact thresholds first |
| `memory.py:197 project_knowledge`, `:286 memory_path`, skill APIs `:496+` | Project/user memory, skills, bundles, lifecycle/evaluation | Memory | `vesper-memory::{project,user,skills,bundles,evaluation}` | Split; compatibility/security high |
| `awareness.py` `EpistemicLedger` | Typed records/evidence/freshness/completion certificates | Core/Memory | `vesper-core::awareness` with session serde | Port contract; metadata privacy risk |
| `metacognition.py:293 MetacognitiveController` | Deterministic uncertainty/risk/mode and aggregate profiles | Core/Memory | `vesper-core::metacognition` | Port after verification/awareness; overthinking regression risk |
| `deliberation.py:296 GroundedDeliberation` | Hypotheses, VOI, critic state/redaction | Core/Memory | `vesper-core::deliberation` | Split deterministic state from provider auxiliary service |
| `repository_intelligence.py` `RepositoryIntelligence` | Bounded lazy graph, impact prediction, premortem | Core | `vesper-context::repository` | Redesign index/graph, preserve bounds/privacy |
| `meta_learning.py:144 SafeMetacognitiveLearning` | Typed causal attribution, drafts, evaluation-gated promotion | Memory | `vesper-memory::meta_learning` | Port late; highest logical/test complexity |
| `telemetry.py:57 TrajectoryRecorder` | Private metadata-only JSONL append | Observability/Persistence | `vesper-observability::events` | Preserve schema reader; redesign sink |
| `observability.py:58 observability_snapshot` | Malformed-tolerant aggregate report | Observability | `vesper-observability::aggregate` | Port after event contract |
| `failure_corpus.py:43 FailureCorpus` | Metadata-only failure drafts and explicit test promotion | Memory/Observability | `vesper-memory::failure_corpus` | Port late; redaction gate |

## MCP, workers, checkpoints, automation, and plugins

| Python path / evidence | Responsibility/callers | Category | Proposed Rust location | Disposition/risk |
|---|---|---|---|---|
| `mcp.py:236 McpManager` | Preset/configured HTTP/stdio MCP lifecycle, recovery, calls | MCP | `vesper-mcp::{manager,transport,presets}` | Redesign atop official Rust SDK where compatible; recovery/name risk |
| `worktrees.py:21 WorktreeManager` | Detached implementation workers, digest verification/promotion | Workers | `vesper-workers::worktree` | Port transactionally; Git conflict risk |
| `worktree_session.py:76 create_worktree_session` | User-facing parallel worktree sessions | Frontend/Workers | `vesper-workers::session` | Merge with worker worktree primitives |
| `checkpoints.py:142 CheckpointManager` | Content-addressed snapshots, policies, rollback/migration/GC | Persistence/Security | `vesper-checkpoints::{store,manifest,rollback,gc}` | Separate crate justified; preserve schema 1/2 |
| `cron.py:271 create_job`, `:487 claim_due` | Versioned jobs, cross-process claims, artifacts | Automation/Persistence | `vesper-automation::{schedule,store,claim}` | Port state machine/schema |
| `cron_scheduler.py:197 run_claimed`, `:290 tick`, `:317 daemon` | Isolated scheduled execution/renewal/watchdog/delivery | Automation | `vesper-automation::runner` | Split from core; cancellation/claim risk |
| `cron_cli.py` | Cron command surface | Frontend/Automation | `vesper-cli::cron` | Port typed commands |
| `plugins.py:134 PluginRegistry` | Signing/trust/data-only install/integrity | Security/Persistence | `vesper-plugins::{manifest,trust,store}` | Separate optional crate; signature/hash risk |
| `plugin_runtime.py:27 PluginRuntime` | Exposes plugin commands/prompts/workflows to TUI/runtime | Core/Frontend | `vesper-plugins::runtime` via capability registry | Redesign event contribution, no TUI coupling |
| `plugin_cli.py` | Plugin CLI surface | Frontend | `vesper-cli::plugin` | Port |

## Static assets and repository support

| Path | Treatment |
|---|---|
| `glm_acp/_mobile_pwa/*` | Preserve user behavior but repackage as embedded optional assets; security review CSP/pairing flow |
| `benchmarks/*` | Convert outputs/cases into cross-harness differential fixtures; retain Python runner until GLM parity |
| `.glm-acp/skills/*` | Repository-owned workflow knowledge; not runtime production code; manually reconsider for Rust repo |
| `registry/*`, `scripts/*`, `.github/workflows/*` | Redesign for Rust binaries while preserving five target names/aliases/checksum/provenance/install contracts |

## Dependency direction

Proposed acyclic direction:

```text
foundational contracts/config/security
        ↓
provider ports, persistence ports, tool ports
        ↓
core runtime/agent loop
        ↓
ACP adapter, CLI, TUI, automation host
```

Providers depend on provider contracts, never core. Tools depend on capability/security contracts, never ACP/TUI. Persistence serializes stable domain DTOs, not frontend/provider client objects. Frontends consume a bounded event stream and issue commands through a runtime facade.

## Retire/redesign candidates requiring approval

- Python self-uninstall implementation and PyInstaller-specific release logic: retire after Rust installer parity.
- Direct `mcp.json` non-atomic writes: strengthen.
- Unversioned session JSON: add a versioned envelope while retaining a legacy reader.
- Shell-string-only `run_command`: add argv-native execution and preserve explicit shell behavior behind policy.
- `agent.py` command monolith and `tui.py` business-state access: split.
- Provider quota assumptions in generic session/UI state: move to provider extension/capability events.
- GLM-specific config IDs (`thought_level`, API plan) in core: normalize capabilities but preserve ACP compatibility adapter during migration.

## Completion status

Every tracked production Python module and static runtime asset family is assigned a responsibility, category, Rust location, treatment, and parity risk. This map intentionally does not authorize implementation.
