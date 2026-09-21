# ADR 0031 — Conversation-owned MCP stdio connections

## Status

Accepted repair refinement of ADR 0013's per-call subprocess lifecycle.

## Context

A fresh subprocess for each MCP call discards Playwright browser state after
navigation. Stateless discovery alone cannot establish interactive readiness.

## Decision

- `McpClient` remains a one-shot compatibility API. Model-facing hosted calls use
  `McpSession`, an explicit conversation owner, never a process-global cache.
- Discovery, browser presets, explicit MCP calls and deferred prefix calls share
  the owner's initialized stdio connection and increasing request IDs.
- Independent owners never share a browser merely because configurations match.
  ACP retains owners by session ID; TUI retains its owner across turns and resets
  it before loading another conversation. Worker services receive fresh owners.
- At most 16 connections are retained per owner. Requests use bounded channels,
  bounded bytes and a deadline of at most 60 seconds. Concurrent requests refuse
  as busy rather than queue without a bound. Cancellation is checked during I/O.
- Transport failure kills/reaps the direct child and quarantines its entry until
  explicit close/reset. Never automatically replay potentially effective actions.
  A changed configuration refuses reuse until explicit close. A successful
  `browser_close` releases the connection; closing an unused owner starts nothing.
- Unscoped discovery retains successful servers and reports per-server errors.
  Scoped discovery fails truthfully. Server `isError` results are tool failures.

## Compatibility and migration

No registry format, credential, dependency or activation change is required.
HTTP retains its existing bounded per-call transport, not persistent stdio
semantics. Existing installed binaries are not replaced by this source repair.
ADR 0013's plugin signature and release-erasure contracts remain unchanged.

## Security consequences

Existing permission gates and worker restrictions remain authoritative. No
personal browser profile or process-global state is introduced. Direct-child
RAII is not proof of arbitrary descendant-process cleanup, nor of reversing
external effects after cancellation. Persistent stdio retains resources until
close/reset/owner destruction; ACP owners otherwise live until engine shutdown.

## Verification and evidence

- `cargo test -p vesper-mcp`
- `cargo test -p vesper-harness --lib mcp`
- `cargo test -p agent-vesper-acp --lib mcp_owners`
- `cargo test -p agent-vesper-tui --bin agent-vesper-tui mcp_tui_wrapped`
- Opt-in real Playwright test in `crates/vesper-mcp/src/playwright_live_tests.rs`.
- [Execution report](../foundation/mcp-session-lifecycle-repair.md).

Linux local receipts are not Windows/macOS or release acceptance.
