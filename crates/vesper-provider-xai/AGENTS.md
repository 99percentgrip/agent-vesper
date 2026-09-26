# Native xAI adapter

## Purpose

Own xAI-specific authentication, catalog, Responses transport, stream decoding,
continuation state, and provider error mapping without changing the shared agent loop.

## Ownership

- `src/auth.rs` and `src/credentials.rs` own fixed-origin Grok browser and
  device authentication, signed-token validation, locked refresh/logout and
  explicit billing-mode selection. Browser login uses the current public native
  client contract, a truthful `agent-vesper` referrer, loopback callback, PKCE,
  state and nonce. The adapter never impersonates or launches Grok Build and
  never reads Grok Build/browser credentials.
- `src/catalog.rs` owns evidence-backed model capabilities; `src/discovery.rs`
  intersects authenticated availability with exact models/current aliases and
  keeps unknown or endpoint-excluded identifiers non-executable.
- `src/wire.rs` owns xAI Responses request/event translation.
- `src/transport.rs` owns bounded HTTP/SSE and WebSocket dispatch,
  cancellation, and Global/API-key native compaction.
- Grok-account inference/model discovery use the first-party session proxy and
  stay isolated from API-key billing.

## Local Contracts

- Grok-session surfaces exclude Global/API-key-only hosted tools, native
  compaction and WebSocket controls. Stale persisted values never cross that
  authentication/billing boundary.
- Pre-dispatch request rejections identify a bounded safe stage and item. They
  never echo prompts, tool arguments, schemas, paths or credentials.
- Current language-model images are JPEG/PNG with a 20 MiB per-item bound and
  no invented item cap. Grok 4.3/4.5 `xhigh` remains fail-closed while official
  model-detail and general-reasoning documentation conflict.
- Catalog identities distinguish canonical, moving, fixed, retired redirect
  and unknown future models; discovery alone never grants capability metadata.

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
  Host controls persist adapter-namespaced values; `hosted_tool_selections`
  alone projects them to request selections, and `wire` remains the final
  fail-closed validator.
- Secrets never enter errors, events, logs, or model-visible extensions.
- No silent fallback between Grok-session and xAI API-key billing paths.
- Grok-session inference sends the pinned first-party proxy protocol headers:
  token-auth, authenticate-response, protocol version, truthful Vesper client
  identity/mode, model override, and bounded request/conversation/session/agent
  correlation. Changing the pinned protocol version requires renewed upstream
  source evidence plus a loopback header regression.
- Authentication mode constrains projected capabilities. Grok-session hosts
  expose only capabilities verified on the session proxy; API-region,
  WebSocket, native compaction, and provider-hosted tools remain API-key-only
  until separate first-party and live evidence establishes session support.
- Host composition uses authenticated discovery before offering models. Memory
  extraction uses the same native session and selected billing mode as the
  active xAI provider; it never falls through to another credential class.

## Verification

- Run `cargo test -p vesper-provider-xai --all-features`.
- Run strict Clippy for `vesper-provider-xai` and `cargo xtask architecture`.

## Child DOX Index

No children.
