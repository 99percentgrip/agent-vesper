# Native xAI adapter

## Purpose

Own xAI-specific authentication, catalog, Responses transport, stream decoding,
continuation state, and provider error mapping without changing the shared agent loop.

## Ownership

- `src/credentials.rs` owns Vesper-managed xAI credentials. It never reads Grok
  Build or browser credential stores.
- `src/catalog.rs` owns evidence-backed model capabilities; discovery establishes
  availability and never invents capabilities.
- `src/wire.rs` owns xAI Responses request/event translation.
- `src/transport.rs` owns bounded HTTP/SSE dispatch and cancellation.
- Grok-account session authentication and proxy transport remain a later VRO-18
  phase and must stay isolated from API-key billing.

## Local Contracts

- Depend only on provider foundations plus HTTP/runtime utility crates.
- Production endpoints are fixed, TLS-only, and redirect-free. Loopback routes
  exist only behind `integration-test-harness`.
- xAI function calls execute through the shared Vesper tool loop.
- Opaque encrypted reasoning may be preserved but never interpreted or exposed.
- Secrets never enter errors, events, logs, or model-visible extensions.
- No silent fallback between Grok-session and xAI API-key billing paths.

## Verification

- Run `cargo test -p vesper-provider-xai --all-features`.
- Run strict Clippy for `vesper-provider-xai` and `cargo xtask architecture`.

## Child DOX Index

No children.
