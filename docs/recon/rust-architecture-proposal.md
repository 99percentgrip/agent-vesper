# Rust Architecture Proposal

Status: COMPLETE

## Architectural verdict

Use a Rust workspace with a small domain kernel, ports/adapters, a single provider-neutral turn engine, and event-driven frontends. Do not mirror the 7,235-line Python `agent.py`. The source evidence for decomposition is the direct import fan-in at `agent.py:56-145` and its combined ACP/loop/policy/command/persistence responsibilities.

## Recommended workspace

### Foundation

- `vesper-domain`: IDs, messages, content parts, tool calls/results, usage, finish/errors, session/goal/plan DTOs, normalized events. No I/O, SDK, UI, or provider dependencies.
- `vesper-config`: profiles, typed global/provider config, secret references, atomic private persistence.
- `vesper-provider`: provider ports, capability model, normalized streams, conformance test kit.
- `vesper-security`: path capabilities, environment scrubber, redaction, promptware, sandbox/process policy primitives.
- `vesper-policy`: ordered policy documents and permission-decision state machine.

### Runtime and services

- `vesper-core`: one turn engine, loop guards, completion state machine, orchestration facade. Depends only on domain/provider/tool/policy/context service traits.
- `vesper-context`: project discovery, progressive rules, references, compaction inputs, repository intelligence.
- `vesper-tools`: schema registry/JIT search and built-in executors; feature modules for filesystem/search/process/workflow/hooks/diagnostics.
- `vesper-sessions`: session repository, legacy JSON codec, SQLite search index and migrations.
- `vesper-checkpoints`: content-addressed store and conflict-aware rollback.
- `vesper-memory`: project/user memory, skills/bundles, awareness/metacognition/deliberation/meta-learning/failure corpus. It may later split if compile/domain pressure warrants.
- `vesper-workers`: bounded read-only delegates and isolated worktree promotion.
- `vesper-mcp`: configured/preset servers, discovery, recovery, tool-name routing.
- `vesper-automation`: schedules, claims, runner, artifacts and delivery.
- `vesper-plugins`: optional data-only packages, trust/signature, contributions.
- `vesper-observability`: metadata events, JSONL compatibility and aggregates.
- `vesper-runtime`: composition root/library facade wiring services; no domain logic.

### Providers

- `vesper-provider-glm`
- `vesper-provider-openai`
- `vesper-provider-anthropic`
- `vesper-provider-gemini`
- `vesper-provider-openai-compatible` (LM Studio is a profile/contribution, not duplicated transport)

### Adapters/frontends

- `vesper-acp`: official ACP SDK adapter and exact wire mapping.
- `vesper-cli`: executable, setup/status/plain/JSON commands.
- `vesper-tui`: reducer, views, terminal/media/voice/mobile adapters. Business state arrives only as runtime events.
- optional `vesper-approval-mobile` if the embedded server remains large.
- `xtask`: fixture generation, compatibility audits, packaging/release automation.

This is fewer boundaries than the user’s illustrative list where cohesion supports it: awareness/deliberation/meta-learning live initially in `vesper-memory`; diagnostics in `vesper-tools` or a small internal crate; mobile/plugins remain optional. Cargo features must not create different domain schemas.

## Dependency graph

```text
vesper-domain     vesper-config     vesper-security
      ↑                 ↑                 ↑
vesper-provider   vesper-policy      persistence/tool ports
      ↑                 ↑                 ↑
provider adapters ──→ vesper-core ←── tools/context/sessions/memory
                          ↑
                    vesper-runtime
             ┌────────────┼────────────┐
          vesper-acp   vesper-cli   vesper-tui
```

Rules:

- no provider adapter depends on core;
- no frontend depends on a concrete provider;
- no persistence schema contains HTTP client/runtime/UI objects;
- tools receive explicit capability handles, not global state;
- cross-service communication uses immutable commands/events and scoped actors, not a shared mega-lock;
- one session actor serializes mutations; independent reads use bounded tasks; provider stream and tool children have hierarchical cancellation tokens.

## State/concurrency model

- `RuntimeSupervisor` owns process lifetime and `JoinSet`s for background MCP/cron/session actors.
- One `SessionActor` owns mutable session state and processes commands serially. Snapshots are immutable `Arc` values for UI reads; avoid `Arc<Mutex<Session>>`.
- Provider response is a bounded channel/stream into the actor. Backpressure is explicit; user-visible deltas must not be dropped.
- Each turn creates a child cancellation scope; provider request, tool process, delegates, diagnostics, and hooks inherit it.
- Tool batches preserve call order. Only operations declared `ConcurrentRead` may run together.
- Persistence writes receive a revision/generation and use compare-and-swap/transaction boundaries to prevent stale overwrites.

## Rust technology assessment

Recommendations are provisional pins; Cargo versions must be locked during foundation. Current documentation checked:

- Tokio 1.49 documents `JoinSet`, `select!`, process `kill`, and `kill_on_drop`; child kill still does not replace explicit process-group/Job Object supervision.
- Reqwest 0.12 defaults client timeouts to none, exposes chunk streaming without the `stream` feature, and requires manual byte limits; therefore set explicit timeouts and use a harness-owned SSE parser to preserve partial-output/no-replay semantics.
- Ratatui 0.30 has Crossterm and in-memory `TestBackend`; terminal restoration remains an application responsibility.
- The official ACP Rust crate is currently 2.0.0 while the stable wire protocol is 1; crate/schema artifact versions are not wire versions and its dispatch ordering warns that callbacks can block the loop.
- The official MCP Rust SDK `rmcp` is current but has had a 1.x migration; pin and wrap it behind `vesper-mcp`.

Docs: [Tokio](https://docs.rs/tokio/1.49.0/tokio/), [Reqwest](https://docs.rs/reqwest/0.12.28/reqwest/), [Ratatui](https://docs.rs/ratatui/0.30.2/ratatui/), [official ACP repository](https://github.com/agentclientprotocol/agent-client-protocol), [official MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk).

| Requirement | Candidate | Why / source replacement | Risk/platform | Status |
|---|---|---|---|---|
| Async runtime/process/I/O | `tokio` | replaces asyncio, tasks, streams, timeouts | process-tree logic still custom | Mandatory |
| Hierarchical cancellation | `tokio-util::sync::CancellationToken` | provider/tool/worker cancellation | verify pinned API | Mandatory |
| HTTP/TLS | `reqwest` + rustls | replaces httpx; explicit timeout/chunk stream | proxy/cert-store differences | Mandatory for HTTP providers |
| SSE | small internal parser over bytes/chunks; optionally `eventsource-stream` only after conformance | exact GLM malformed/partial/no-auto-reconnect behavior | UTF-8/chunk boundaries | Internal mandatory |
| Serialization/schema | `serde`, `serde_json`, `schemars` | ACP/provider/session/tool schemas | deny-unknown vs forward compatibility | Mandatory |
| ACP | official `agent-client-protocol` crate | replaces Python SDK | fast-moving 2.0 Rust API; wire v1 distinction | Mandatory, wrapped |
| CLI | `clap` | argparse/help/subcommands | exact help/output drift | Mandatory |
| TUI/events | `ratatui`, `crossterm` | Textual and terminal events | accessibility/selection/mouse parity | Mandatory for TUI |
| SQLite/FTS | `rusqlite` with controlled bundled/system feature, or `sqlx` if async transactions prove useful | sqlite3 FTS/session index | bundled FTS/platform packaging | Mandatory; choose after spike |
| Full-text | SQLite FTS5, not a new search engine | preserves index semantics | tokenizer parity | Mandatory |
| Cron/timezone | `cron`/`croner` evaluation spike + `chrono`/`chrono-tz` (or `time` + TZ crate) | croniter/zoneinfo | DST and five-field semantics vary | Mandatory after golden cases |
| Crypto/signatures | `ed25519-dalek`, `sha2`; standard library/`flate2` for SHA-1 Git IDs/zlib | plugin/checkpoint hashes/signatures | crypto audit/features | Mandatory for plugins/checkpoints |
| Secret types/keyring | `secrecy`, `zeroize`, optional `keyring` | credential non-Debug and OS storage | headless/keyring availability | secrecy mandatory; keyring optional |
| Filesystem traversal/ignore/glob | `ignore`, `walkdir`, `globset` | rg fallback, gitignore, references/checkpoints | symlink/encoding differences | Mandatory |
| Regex | `regex`; separate bounded/safe policy for user regex | Python re/policy/search | no backreferences; DoS policy | Mandatory |
| Process platform APIs | `nix`/`rustix` and `windows-sys` narrowly | setsid/killpg/Job Objects | unsafe code review | Mandatory platform modules |
| Logging | `tracing`, `tracing-subscriber` | logging + metadata telemetry separation | secret fields | Mandatory |
| Errors | `thiserror`; `anyhow` only at binary boundary | typed provider/tool/security errors | accidental source leakage | Mandatory/limited |
| Config | typed serde + atomic writer; consider `figment` only if layered sources justify it | config.py/profile files | hidden precedence | Internal first |
| LSP | `lsp-types` plus supervised JSON-RPC process; evaluate `tower-lsp` only for server use | current custom client | process lifecycle more important than framework | Optional |
| MCP | official `rmcp`, wrapped | custom HTTP/stdio manager | SDK maturity/migrations | Optional feature, required for parity |
| Property/snapshot | `proptest`, `insta` | parsers/state reducers/goldens | snapshot review discipline | Dev mandatory |
| Fuzz/fault | `cargo-fuzz`, `loom` selectively, injectable filesystem/process/provider fakes | resilience.py and concurrency | CI cost/platform | Dev optional/targeted |
| Test runner | built-in tests + `cargo-nextest` | pytest suite execution | platform process semantics | Dev recommended |
| Packaging | Cargo release profiles; evaluate `cargo-dist`/native installers after artifact spike | PyInstaller/release workflow | five-target installer contract | Later |

No dependency should be added merely because it appears here; each enters only with its owning stage and an ADR recording features/MSRV/license/audit.

## Persistence boundaries

- Stable domain DTO versions live in `vesper-domain`.
- `vesper-sessions` owns legacy Python JSON decoding and new schema migrations.
- Stores use repositories/transactions; core never opens paths or SQL.
- SQLite index is derived and rebuildable.
- Provider metadata is namespaced opaque data with redaction.
- Checkpoint blobs and plugin trust are separate stores with stricter security APIs.

## Frontend boundary

`vesper-runtime` emits a monotonically sequenced `HarnessEvent { session_id, turn_id, seq, payload }`. ACP maps it to wire updates; TUI reduces it into view state; CLI renders it. Frontends issue `HarnessCommand`s and return permission decisions. This prevents direct TUI/provider coupling and makes reducer/event-order differential tests possible.

## Provider boundary

The provider interface uses normalized request/stream/capability/error types described in `provider-abstraction-analysis.md`. Advanced features are capability-driven. Provider-specific config/metadata remains versioned opaque data.

## Feature flags and binaries

- Default binary includes ACP, CLI, GLM, OpenAI-compatible, core tools and TUI.
- Anthropic/OpenAI/Gemini direct adapters may be default once stable; voice/mobile/plugins/keyring/platform sandboxes are explicit features if packaging impact is material.
- Feature matrices are tested; disabled features return observable unsupported status rather than removing schema fields unpredictably.

## Architecture decision gates

Before crate creation:

1. Freeze domain/ACP/provider fixture schemas.
2. Spike official ACP 2.0 crate against protocol-v1 Python transcripts.
3. Spike reqwest custom SSE cancellation/partial EOF.
4. Spike SQLite FTS5 packaging on five targets.
5. Spike process-tree cleanup on Linux/macOS/Windows.
6. Decide MSRV and supported target triples.

## Completion status

Workspace boundaries, dependency direction, concurrency/state ownership, technology choices, platform implications, frontend/provider/persistence ports, and pre-foundation gates are defined. No crate has been created.
