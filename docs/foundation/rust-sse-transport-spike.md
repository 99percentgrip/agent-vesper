# Rust HTTP/SSE Transport Spike

Status: COMPLETE

## Objective

Validate the HTTP/SSE primitives required by a future GLM adapter without
implementing that adapter.

## Documentation and dependency evidence

Context7 and current package metadata show:

- reqwest defaults connect/read/overall timeouts to none unless configured;
- response chunk streaming is available directly;
- automatic SSE reconnection is not provided when using raw response chunks;
- current published reqwest is 0.13.4 (Rust 1.85.0), newer than the
  reconnaissance-era 0.12.28.

The spike pins reqwest 0.13.4 with default features disabled and explicit
`json` + `rustls`, Tokio 1.52.0, tokio-util 0.7.17, serde_json 1.0.145, and
httpdate 1.0.3. The client explicitly sets connect, request, and read timeouts
and `retry(reqwest::retry::never())`.

## Method

`spikes/rust-sse-transport/` contains a disposable parser over
`Response::chunk()` and a raw loopback TCP server. The parser:

- buffers bytes until full LF/CRLF lines, so split UTF-8 is decoded only after
  reassembly;
- ignores blank/comment/non-`data:` and malformed JSON lines like Python
  `glm_client.py:691-711`;
- preserves reasoning, content, tool-index fragment, usage, finish, and DONE
  order;
- records whether visible output occurred for EOF/transport errors;
- checks cancellation around send and every body chunk;
- bounds a line to 4 KiB and a response body to 64 KiB in the spike;
- performs no reconnect or retry.

The future production values must be selected from real provider/tool schema
limits; the spike constants only prove enforcement.

## Cases and results

`cargo test --locked`: **10 passed, 0 failed**, 0 doc tests, 0.69s.

Covered locally:

- byte-by-byte segmentation and UTF-8 split;
- CRLF/LF, blank/comment/non-data lines;
- malformed JSON and `[DONE]`;
- reasoning→content and terminal finish order;
- fragmented/interleaved tool indexes and usage-only data;
- EOF with and without visible output;
- declared-length transport failure before/after visible output;
- retryable status surface and numeric/date `Retry-After` capped parsing;
- cancellation before request, during delayed headers/body, and during a
  continuation request;
- no post-cancel event and preservation of first-continuation output;
- explicit read timeout;
- line/body memory limits;
- one accepted connection (no automatic reconnect);
- consumption of the Python GLM reasoning/content fixture.

The Python corpus separately covers output-length continuation and its 20-call
cap. The Rust spike deliberately leaves continuation policy to the provider
adapter; it proves cancellation/partial-output transport semantics across a
second request.

## Verdict

**Suitable for the future adapter behind a harness-owned parser.**

Required production rules:

- keep reqwest retries disabled and implement status retry policy above it;
- never retry/replay after visible output;
- cancellation wins over send/read/continuation and drops the response;
- one terminal outcome, no post-cancel delta;
- parse/retain `Retry-After` before mapping HTTP errors;
- set explicit connect/request/read limits;
- choose reviewed production line/body/tool-argument bounds;
- keep provider continuation in `vesper-provider-glm`, not transport/core.

## Commands and files

Commands:

- Context7 reqwest documentation query and `cargo info reqwest`;
- initial `cargo test` to resolve the lock;
- repeated `cargo test --locked`, final 10/10 pass.

Created:

- `spikes/rust-sse-transport/{AGENTS.md,README.md,Cargo.toml,Cargo.lock}`
- `spikes/rust-sse-transport/src/lib.rs`
- this report.

## Platform scope, unresolved issues, readiness

Proof is Linux x86-64 only. TLS roots/proxies, DNS/connect-timeout behavior,
macOS/Windows cancellation, and MSRV 1.88 builds are CI pending. The local
transport assumptions are resolved sufficiently for workspace foundation;
the production GLM adapter remains unauthorized.

