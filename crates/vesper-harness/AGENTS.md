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

- `src/lib.rs` owns the shared service, bounded durable-store wiring, and
  provider-worker delegation boundary.
- Frontends own provider selection, approval UI, ACP/TUI protocol mapping, and
  session lifecycle; they inject a `WorkerFactory` when nested work is allowed.

## Local Contracts

- Every filesystem path is confined to the caller's primary workspace root
  before access.
- Durable roots are supplied by the composition boundary and default to the
  `.agent-vesper/` layout; no credentials are persisted by this crate.
- The service exposes the same hosted tool definitions and behavior to ACP and
  TUI, avoiding frontend-specific parity drift.
- First-party MCP presets are not persisted and cannot be shadowed by custom
  registry entries. Web and reader calls use the configured Z.ai API-key
  environment; vision paths are workspace-confined; browser actions are an
  explicit allowlist and never arbitrary JavaScript evaluation.
- All output, source scans, batches, workflow depth, and worker actions remain
  bounded. When a provider-backed worker is supplied, the host starts a
  one-second polling scheduler that claims due cron jobs, executes them, and
  persists bounded status/output; dropping the service aborts that scheduler.
- No protocol, provider-wire, UI, SQLite, or live-provider dependency is
  allowed here.

## Work Guidance

- Add provider-neutral core tools to `vesper-agent`; add durable or host-bound
  tools here and inject them through `ToolRegistry::with_service`.
- Preserve the fail-closed permission gate in the agent loop; this service
  never bypasses it.

## Verification

- Run `cargo test -p vesper-harness`.
- Run `cargo xtask architecture` and `cargo xtask verify`.

## Child DOX Index

No children.
