# ACP protocol adapter

## Purpose

Own official ACP Rust SDK integration, protocol-v1 compatibility, request and
update mapping, and bounded asynchronous dispatch into `vesper-runtime`.

## Local Contracts

- ACP SDK and wire types remain inside this crate.
- SDK callbacks enqueue bounded work and never await provider completion.
- The compatibility layer owns legacy `PromptResponse.userMessageId` placement.
- Stdout transport carries ACP JSON-RPC only.
- Session updates must reach the physical writer through bounded flow control;
  SDK-internal queues may not defeat runtime backpressure.
- Load/resume replay is visible history, plan, metadata/mode, available
  commands, then lifecycle response; every update is writer-accepted first.
- A successful prompt turn is persisted through the injected runtime writer
  before the prompt response is sent; the save runs inside the prompt's
  detached task so the dispatcher loop never blocks. A persistence failure
  surfaces a sanitized request error with a stable reason and the dispatcher
  continues serving other requests.
- The crate is provider-neutral, maps persistent read outcomes through runtime,
  and performs no direct persistence I/O.
- Corrupt, unsupported, bounded, denied, unsafe, workspace-mismatch, and
  write-failed records return sanitized request errors without terminating
  dispatch.
- Session mode/config requests are mapped to runtime mode, reasoning, and
  permission updates; delete and logout have explicit protocol responses.
- `AcpPromptEngine` is an optional composition port. When injected, prompt
  requests route through a host's bounded multi-turn `vesper-agent` loop and
  are published with ACP backpressure. Cancellation notifications are routed
  to the injected engine as well. The adapter supplies a bounded
  `session/request_permission` bridge to injected engines; cancellation
  resolves any pending approval and the engine must fail closed on rejection
  or a missing bridge. Without an injected engine, the runtime single-turn
  path remains available for protocol conformance.

## Verification

- Run `cargo test -p vesper-acp --all-features`.
- Run `cargo xtask acp verify` and architecture checks.
- Run the slow-reader and cancellation-under-pressure process tests.

## Child DOX Index

No children.
