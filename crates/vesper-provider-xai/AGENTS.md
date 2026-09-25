# Native xAI adapter

## Purpose

Own xAI-specific authentication, catalog, Responses transport, stream decoding,
continuation state, and provider error mapping without changing the shared agent loop.

## Ownership

- `src/auth.rs` and `src/credentials.rs` own fixed-origin browser/device Grok
  authentication, signed-token validation, locked refresh/logout and explicit
  billing-mode selection. They never read Grok Build or browser credentials.
- `src/catalog.rs` owns evidence-backed model capabilities; `src/discovery.rs`
  intersects authenticated availability with exact models/current aliases and
  keeps unknown or endpoint-excluded identifiers non-executable.
- `src/wire.rs` owns xAI Responses request/event translation.
- `src/transport.rs` owns bounded HTTP/SSE and WebSocket dispatch,
  cancellation, and Global/API-key native compaction.
- Grok-account inference/model discovery use the first-party session proxy and
  stay isolated from API-key billing.

## Local Contracts

- Depend only on provider foundations plus HTTP/runtime utility crates.
- Production endpoints are fixed, TLS-only, and redirect-free. Loopback routes
  exist only behind `integration-test-harness`.
- xAI function calls execute through the shared Vesper tool loop.
- Opaque encrypted reasoning may be preserved but never interpreted or exposed.
- Stored Responses continuation and prompt-cache routing are explicit,
  bounded `provider.xai` controls; continuation never silently enables remote
  retention.
- WebSocket is an explicit Global/API-key transport optimization with one
  serialized turn per connection. Handshake failure may fall back to HTTP
  before dispatch; send/stream failure never replays ambiguous work.
- Native compaction preserves the returned encrypted item unchanged. Shared
  AgentLoop policy remains default-off and owns transactional replacement.
- Provider citations remain distinct from Vesper web-tool events and are not
  replayed as provider input.
- xAI hosted tools are explicit Global/API-key-only selections. Attachment,
  collection and Remote MCP configuration is bounded and non-secret; Remote
  MCP accepts HTTPS endpoints without embedded credentials. These tools never
  map to Vesper `run_command`, web or MCP execution.
- Secrets never enter errors, events, logs, or model-visible extensions.
- No silent fallback between Grok-session and xAI API-key billing paths.
- Host composition uses authenticated discovery before offering models. Memory
  extraction uses the same native session and selected billing mode as the
  active xAI provider; it never falls through to another credential class.

## Verification

- Run `cargo test -p vesper-provider-xai --all-features`.
- Run strict Clippy for `vesper-provider-xai` and `cargo xtask architecture`.

## Child DOX Index

No children.
