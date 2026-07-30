# Official Rust ACP SDK Protocol-v1 Spike

Status: COMPLETE

## Objective

Determine whether the current official Rust ACP SDK can reproduce frozen
Python ACP protocol-v1 behavior without implementing Agent Vesper logic.

## Documentation and package evidence

Context7 resolves the official SDK to
`agentclientprotocol/rust-sdk`. Its main-branch documentation lagged published
metadata and reported crate 1.2.0; crates.io and `cargo info` are authoritative
for the spike:

- crate: `agent-client-protocol = 2.0.0`;
- declared Rust version: 1.88.0;
- underlying v1 schema package pinned by the crate: 1.5.0;
- stable wire version: `ProtocolVersion::V1` (numeric `1`);
- session fork is behind `unstable_session_fork`;
- draft protocol v2 is a separate `unstable_protocol_v2` feature and is not
  enabled.

Crate documentation explicitly states that ordered request/notification
callbacks hold the inbound dispatch loop. Awaiting later inbound traffic in
such a callback can deadlock; work must be spawned/enqueued and the callback
must return.

## Spike method

Created disposable package `spikes/acp-rust-protocol-v1/` with exact pins and
seven tests:

1. consume the Python initialization fixture and serialize numeric wire v1;
2. serialize new/load/resume/fork/list/close/prompt/cancel with stable session
   IDs;
3. preserve thought→message→usage update order and usage field names;
4. reject malformed required fields while confirming SDK default-on-error
   behavior for optional capabilities;
5. exercise in-memory session/new → prompt → update → stop dispatch;
6. prove one ordered callback blocks a later inbound notification;
7. prove clean in-memory shutdown completes under timeout.

The package contains only fake runtime behavior and schema/dispatch tests.

## Commands and results

- `cargo search agent-client-protocol --limit 5`
- `cargo info agent-client-protocol --verbose`
- bounded inspection of downloaded crate 2.0.0, schema 1.5.0, examples,
  ordering concepts, and upstream session-ordering tests.
- `cargo fetch`
- `cargo test --locked`

Result: **7 passed, 0 failed**; doc tests 0; finished in 0.10s after build.

## Compatibility findings

| Area | Finding | Classification |
|---|---|---|
| Initialize and protocol number | Numeric protocol v1 and camelCase fields match | Locally validated |
| New/load/resume/list/close | Required DTOs and wire names available | Locally validated |
| Fork | Available only through an explicitly unstable Cargo feature | Confirmed mismatch/risk |
| Prompt/cancel | Dispatch and stop reason work in memory | Locally validated |
| Update ordering | SDK preserves send/dispatch order; callbacks are barriers | Locally validated |
| Message IDs | Current SDK puts optional `messageId` on `ContentChunk`; frozen Python ACP 0.10.x returns `userMessageId` on `PromptResponse` (`agent.py:2389-2535`) | Confirmed wire-shape mismatch requiring compatibility treatment |
| Malformed fields | Required type mismatch errors; selected optional fields default on error | Locally validated |
| Clean shutdown | In-memory peer completion exits cleanly | Locally validated |
| Stdio/process shutdown | Not exercised in this minimal spike | CI/follow-up validation pending |

The source dependency is `agent-client-protocol>=0.10.1,<0.11`
(`pyproject.toml`, `uv.lock`), so crate API version and Python package version
must never be mistaken for wire protocol version.

## Verdict

**Suitable behind a compatibility wrapper.**

The wrapper must:

- pin 2.0.0 and wire v1 separately;
- isolate the unstable fork feature;
- enqueue runtime commands and immediately release ordered callbacks;
- map source `userMessageId` semantics to the current chunk-message-ID model
  or use a narrowly tested raw/custom response compatibility path;
- own exact update sequencing and malformed-message policy outside business
  logic;
- add process-level JSONL/stdio shutdown tests before production ACP work.

If exact `PromptResponse.userMessageId` cannot be emitted through a supported
extension/custom message path, the classification degrades to “suitable only
with patches”; this is the remaining adapter-specific proof, not a blocker to
the workspace foundation.

## Files created

- `spikes/acp-rust-protocol-v1/{AGENTS.md,README.md,Cargo.toml,Cargo.lock}`
- `spikes/acp-rust-protocol-v1/src/lib.rs`
- this report

## Platform scope and readiness

The tests ran only on Linux x86-64 with Rust 1.95.0. MSRV 1.88 and
macOS/Windows stdio behavior remain CI pending. The SDK is acceptable for
Stage 1 only behind the proposed wrapper boundary; it must not leak directly
into core domain contracts.

