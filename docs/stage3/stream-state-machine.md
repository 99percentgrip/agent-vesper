# GLM Stream State Machine

Status: COMPLETE

## States and guarantees

The operation moves through requesting and streaming to exactly one completed,
cancelled, interrupted, or error terminal result. `sse::SseParser` accepts
arbitrary byte segmentation, LF/CRLF, comments, non-data lines, `[DONE]`, and
UTF-8 split across chunks. Malformed JSON data is ignored for source parity.

Bounds are 256 KiB per line/event, 128 bytes for a tool name, 1 MiB for
accumulated arguments, 64 KiB for provider metadata, and 500 bytes for safe
error prefixes. The parser never retains the complete response.

`response::AttemptState` flushes reasoning before content and both before tool
start, assembles parallel calls by provider index, preserves fragmented IDs,
names, and arguments, emits usage in wire order, and rejects post-terminal
events. Missing IDs receive deterministic request/index-derived IDs.

## Evidence

- Frozen source: `glm_acp/glm_client.py::GlmClient._execute_stream`, lines
  650–803; malformed lines 691–710; ordering 665–678 and 740–762.
- Rust: `sse.rs`, `response.rs`, `transport.rs`, `adapter.rs`.
- Tests: all 21 `fixtures/provider/glm` scenarios execute against a loopback
  raw server; byte-at-a-time Unicode and oversized tool arguments are tested.

The bounded accumulators and deterministic fallback IDs intentionally
strengthen the source without changing successful user-visible semantics.

