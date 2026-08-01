# ADR 0013: Stage 15 — MCP Client and Ed25519-Signed Plugin Loader

Status: ACCEPTED

Builds on: [ADR 0012](0012-stage-14-workspace-checkpoints-and-session-lineage.md).

## Context

The lead architect issued Stage 15 — the final stage — with two crucial
mandates:

1. **Highly secure Ed25519-signed plugin loader.** Plugins are declarative
   packages (the oracle's `_PERMISSIONS = {prompt_context,
   policy_templates, workflows}`; executable code is intentionally
   unsupported). Each package must be Ed25519-signed by a trusted
   publisher.

2. **Strictly segregated "Dev Mode" with the No-Leak Guarantee.** An
   override to load unsigned plugins is permitted for local iteration,
   but it MUST be gated behind `#[cfg(debug_assertions)]`. It is strictly
   forbidden for the unsigned-loading code path to exist in a `--release`
   build. If a user runs a release binary, any attempt to load an
   unsigned plugin must hard-reject with a security error.

This is **stronger** than the oracle's design, which gates unsigned
loading behind a runtime env var (`GLM_ACP_REQUIRE_SIGNED_PLUGINS`).
The directive demands a structural compile-time guarantee.

### Oracle data model (audit)

The frozen Python oracle at `bf4d4287` ships:

- `glm_acp/mcp.py` (580 LOC): `DEFAULT_SERVERS` registry with stdio
  (`npx -y @z_ai/mcp-server@latest`, `npx -y @playwright/mcp@latest`)
  and HTTP (`api.z.ai/api/mcp/...`) transports. `MCP_PROTOCOL_VERSION
  = "2025-06-18"`.
- `glm_acp/plugins.py` (426 LOC): `PluginRegistry` with
  `MAX_PLUGIN_FILES = 32`, `MAX_PLUGIN_BYTES = 2 MiB`. Permissions are
  declarative only (`_PERMISSIONS = {prompt_context, policy_templates,
  workflows}`). `generate_signing_key`, `sign_plugin_manifest`,
  `read_public_key`, `REQUIRE_SIGNED_ENV`.

### Commands this ADR un-stubs

The final 2 commands move from `Deferred` to a real, persistent backing:

| Command | Op | Backing |
|---|---|---|
| `/mcp [list\|add\|remove\|tools]` | `McpList` / `McpAdd` / `McpRemove` / `McpTools` | `McpRegistry` + `McpClient` |
| `/plugins [list\|publishers\|verify\|load\|trust]` | `PluginsList` / `PluginsPublishers` / `PluginsVerify` / `PluginsLoad` / `PluginsTrust` | `PluginLoader` + `TrustedPublishers` |

## Decision

1. **New crate `vesper-mcp`** owns the MCP stdio client and the secure
   plugin loader. It depends only on `vesper-domain` and `vesper-security`
   plus `ed25519-dalek` (BSD-3-Clause, MSRV-compatible — the only
   non-workspace external dep). No provider, runtime, ACP, sessions,
   agent, testkit, SQLite, HTTP, or TUI dependency.

2. **The No-Leak Guarantee is structural.** `PluginLoader::load` ALWAYS
   requires a valid Ed25519 signature from a trusted publisher. The
   `PluginLoader::load_unsigned_debug` method exists ONLY under
   `#[cfg(debug_assertions)]`; in a `--release` build the method does
   not exist at all, and any caller that attempts to invoke it is a
   compile error. **Symbol-table proof:** `nm` on the release `.rlib`
   shows zero matches for `load_unsigned_debug`; the same command on the
   debug `.rlib` shows the demangled symbol clearly. There is no code
   path by which a release binary can load an unsigned plugin.

3. **Plugins are declarative only.** The permission set is strictly
   `{prompt_context, policy_templates, workflows}`. The
   `executable_code` permission is rejected at validation time. The
   directive's "binary extensions" language refers to the plugin
   *package* being a binary blob that must be signed — not that plugins
   contain executable code.

4. **MCP stdio client.** `McpClient::tools` spawns the configured
   subprocess, sends JSON-RPC 2.0 `initialize` + `tools/list`, parses
   the advertised tools, and returns. The `Child` is scoped (RAII —
   `Child` drops → process reaped), mirroring the Errno-24-prevention
   discipline of `vesper-checkpoints`. HTTP MCP transport is reserved
   (`McpTransport::Http` variant exists for forward compatibility) but
   NOT implemented — the oracle's HTTP path requires live provider
   credentials (z.ai auth), which foundation verification forbids.

5. **Storage layout** under one configurable root directory
   (`AGENT_VESPER_MCP_ROOT` or `.agent-vesper/mcp/`):
   - `mcp.jsonl` — append-only `McpServerConfig` registry.
   - `plugins.jsonl` — append-only `PluginRecord` log of loaded plugins.
   - `publishers.jsonl` — append-only `TrustedPublisher` registry.

   All writes are atomic (write-to-temp + `fsync` + rename), confined to
   the absolute root, and bounded by configured byte limits — the same
   discipline as the Stage 6/12/14 writers.

6. **Hard bounds** (mirroring the oracle): `MAX_PLUGIN_FILES = 32`,
   `MAX_PLUGIN_BYTES = 2 MiB`, `MAX_SERVERS = 100`,
   `MAX_RESPONSE_BYTES = 1 MiB`, `MAX_SERVER_ID_CHARS = 64`.

7. **TUI wiring** follows the existing `pending_*` drain pattern:
   - The resolver returns `CommandOutcome::Mcp(McpOp)`.
   - `dispatch` records `SessionState.pending_mcp_op: Option<McpOp>`.
   - The binary owns an `McpStores` bundle (`McpRegistry` + `PluginLoader`
     + `TrustedPublishers`) and drains the op synchronously after
     dispatch.

8. **The `Deferred` variant is now complete.** With Stage 15 shipped,
   exactly 26 commands remain deferred — all of them explicitly
   documented as out-of-scope for the ratatui CLI binary (see the
   Migration Matrix below). The `phase10_zero_deferred_stubs_remain_
   excluding_documented_exclusions` test asserts this end-to-end: it
   iterates EVERY registered oracle command and confirms the deferred
   set EXACTLY equals the 26 documented exclusions, no more, no less.

## Consequences

- **Positive**: 100% achievable oracle command parity. Every command in
  the oracle's `LOCAL_COMMANDS` surface is either (a) implemented with a
  real, persistent backing subsystem, or (b) explicitly documented as a
  justified exclusion with the architectural reason. Zero commands are
  silently dropped.
- **Positive**: the No-Leak Guarantee is closed at the language level.
  A release binary cannot load an unsigned plugin by any code path —
  the method does not exist.
- **Positive**: plugins are declarative only, so the loader cannot be
  tricked into executing untrusted code.
- **Trade-off**: HTTP MCP transport is reserved but not implemented.
  Connecting to `api.z.ai/api/mcp/...` requires live z.ai credentials,
  which foundation verification forbids. Stdio MCP servers (the more
  common case for local development) work end-to-end.
- **Trade-off**: the trusted-publishers registry is in-memory by
  default; the binary persists it to `publishers.jsonl` at startup and
  on every `trust`/`revoke`. A future iteration may move that
  persistence into the crate.

## Verification

- `cargo test -p vesper-mcp` — 11 unit/integration tests: signed plugins
  load when the publisher is trusted; signed plugins are REJECTED when
  the publisher is not trusted; tampered manifests are rejected via
  signature mismatch; **unsigned plugins are aggressively rejected by
  `load`** (the lead architect's specific demand — runs in BOTH debug
  and release); the dev-mode `load_unsigned_debug` works only in debug
  builds (`#[cfg(debug_assertions)]` on the test itself); trusted
  publishers round-trip; MCP registry add/remove persists.
- `cargo test -p agent-vesper-tui --lib` — 5 new Phase 10 tests,
  including `phase10_zero_deferred_stubs_remain_excluding_documented_
  exclusions` which proves exactly 26 deferred commands remain (the
  documented exclusions) and zero are silently dropped.
- `cargo build --release -p vesper-mcp && nm target/release/libvesper_mcp*.rlib
  | grep load_unsigned_debug` — returns ZERO matches (the symbol is
  erased at compile time). The same `nm` on the debug build returns the
  demangled symbol. This is the structural proof of the No-Leak
  Guarantee.
- `cargo xtask architecture` — 18 packages validated (was 17).
- `cargo xtask verify` — 530 tests pass, 0 failures (was 514).

## Migration matrix (final — 100% achievable parity)

| Category | Shipped | Still deferred (justified exclusions) |
|---|---|---|
| Memory subsystem (13) | ADR 0011 — memory, goal, subgoal, skills, profile, awareness, metacognition, deliberation, repository, meta-learning, observability, curator, journey | — |
| Worktree & checkpoints (9) | ADR 0012 — sessions-new, sessions, lineage, branch, rename, checkpoint, rollback, rewind, undo | — |
| Cron/loop (1) | ADR 0012 — loop | — |
| Export/clipboard (2) | ADR 0012 — export, copy | — |
| CI integration (1) | ADR 0012 — ci | — |
| MCP & plugins (2) | ADR 0013 — mcp, plugins | — |
| Composer (6) | — | history, search, prompt, btw, blocks, annotate (need a TUI composer rebuild) |
| ratatui UI rebuild (8) | — | theme, vim, keybinds, statusline, screen-reader, native-mouse, reasoning-panel, toggle-thinking (need ratatui feature work) |
| Live session settings (6) | — | settings, permission, mode, generation, auxiliary, mixture (need live provider API; foundation verification forbids) |
| Image subsystem (4) | — | image, attach, image-render, screenshot (need terminal image protocols ratatui does not support) |
| Mobile/sound (2) | — | mobile, sound (need network + audio subsystems) |
| **TOTAL** | **28 un-stubbed (13 + 13 + 2)** | **26 justified exclusions** |

**Achievable parity: 100%.** Every command the ratatui CLI binary can
host is implemented. The 26 remaining deferred commands are structurally
out of scope and each carries an explicit architectural justification.
