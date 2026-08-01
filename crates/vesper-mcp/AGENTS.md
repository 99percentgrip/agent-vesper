# vesper-mcp — MCP client and Ed25519-signed plugin loader

## Purpose

Own the **MCP stdio client** and the **secure plugin loader** (ADR 0013 —
Stage 15). Backs the Tier C Phase 10 un-stubbed TUI commands `/mcp` and
`/plugins`. This is the final stage: with it, every achievable oracle
command is un-stubbed.

The crucial security mandate: the unsigned-plugin loading code path is
**structurally erased from `--release` builds** via
`#[cfg(debug_assertions)]` — not merely gated by a runtime env var like
the oracle's `REQUIRE_SIGNED_ENV`. A release binary cannot load an
unsigned plugin by any code path.

## Ownership

- `src/lib.rs` — public re-exports and crate-level docs.
- `src/error.rs` — `McpError` (sanitized; never leaks paths or payloads).
- `src/mcp.rs` — `McpRegistry` (config-driven server list, JSONL-backed)
  and `McpClient` (bounded JSON-RPC 2.0 over stdio and Streamable HTTP).
- `src/plugins.rs` — `PluginManifest`, `PluginSignature`,
  `TrustedPublishers`, and the security-critical `PluginLoader` with
  `load` (always verifies) and `load_unsigned_debug` (only compiles under
  `#[cfg(debug_assertions)]`).

## Local Contracts

- Depends only on `vesper-domain` and `vesper-security`. No provider,
  runtime, ACP, sessions, agent, testkit, SQLite, or TUI dependency.
  `ed25519-dalek` is the only non-workspace external dep (BSD-3-Clause,
  MSRV-compatible).
- **No-Leak Guarantee.** `PluginLoader::load` ALWAYS requires a valid
  Ed25519 signature from a trusted publisher. `PluginLoader::load_unsigned_debug`
  exists ONLY under `#[cfg(debug_assertions)]`; in a `--release` build
  the method does not exist at all. A release binary therefore CANNOT
  load an unsigned plugin — there is no code path by which it could.
- **Plugins are declarative only.** The oracle's
  `_PERMISSIONS = {prompt_context, policy_templates, workflows}` is
  enforced: executable code is intentionally unsupported. The
  "binary extensions" language in the directive refers to the plugin
  *package* being a binary blob that must be signed — not that plugins
  contain executable code.
- **Subprocess RAII.** `McpClient` spawns the configured stdio server
  with piped stdin/stdout. The `Child` is scoped to the function body;
  when `tools()` returns the child has been dropped and the process
  reaped (mirrors the Errno-24-prevention discipline of
  `vesper-checkpoints`).
- **HTTP MCP transport is bounded and opt-in.** It uses a short timeout,
  caps response bytes, and reads an optional bearer token from a named
  environment variable; the secret is never persisted. Foundation tests use
  no live provider endpoint.
- Protected first-party presets (`zai_search`, `zai_reader`, `zai_vision`, and
  `playwright`) are available to the harness without being persisted or
  replaceable by custom registry entries. Streamable HTTP initialization,
  session headers, notifications, and event-stream responses are handled
  within the bounded client. Playwright subprocesses receive a sanitized
  environment; vision receives `Z_AI_API_KEY` only when its configured auth
  environment is available.
- Hard bounds: `MAX_PLUGIN_FILES = 32`, `MAX_PLUGIN_BYTES = 2 MiB`,
  `MAX_SERVERS = 100`, `MAX_RESPONSE_BYTES = 1 MiB`,
  `MAX_SERVER_ID_CHARS = 64`.
- Stores never create the root directory; the composition boundary
  (binary) is responsible for ensuring it exists.
- No live provider calls.

## Work Guidance

- When adding a new plugin permission, update the `validate()` allowlist
  AND the AGENTS.md "Plugins are declarative only" clause.
- The signature scheme is Ed25519 (not RSA, not ECDSA). The public key
  is stored as 32 bytes hex-encoded in `TrustedPublisher::public_key_hex`.
- The trusted-publishers registry is in-memory by default; the binary
  persists it to `publishers.jsonl` at startup and on every `trust` /
  `revoke`. (A future iteration may move that persistence into this
  crate; for now the binary owns the file.)
- `McpClient::tools` is a synchronous blocking call. The TUI binary
  invokes it on a background thread when the driver asks
  `/mcp tools <name>` so the UI does not freeze.

## Verification

- `cargo test -p vesper-mcp` — unit + integration tests: signed plugins
  load when the publisher is trusted; signed plugins are REJECTED when
  the publisher is not trusted; tampered manifests are rejected via
  signature mismatch; **unsigned plugins are aggressively rejected by
  `load`** (the lead architect's specific demand, runs in BOTH debug
  and release); the dev-mode `load_unsigned_debug` works only in debug
  builds (`#[cfg(debug_assertions)]` on the test itself); trusted
  publishers round-trip; MCP registry add/remove persists.
- `cargo xtask architecture` — confirms the new crate satisfies the
  production dependency allowlist and the source-tree unsafe ban.
- `cargo build --release -p vesper-mcp` — proves the No-Leak Guarantee
  structurally: `load_unsigned_debug` is erased, and any caller that
  attempts to invoke it would be a compile error.

## Child DOX Index

No children.
