# Stage 4 ACP adapter and minimal runtime report

Status: COMPLETE — CI VALIDATION PENDING

## Scope and result

Stage 4 created:

- `crates/vesper-runtime`: provider-neutral ephemeral supervisor/session actors;
- `crates/vesper-acp`: official-SDK ACP v1 adapter and compatibility wrapper;
- `apps/agent-vesper-acp`: thin stderr-traced stdio composition binary.

The production path reaches `vesper-provider-glm` through neutral provider
ports and a deterministic loopback server. No persistent store, agent/tool loop,
process executor, MCP, memory, automation, or frontend was added.

New direct production dependencies are the official ACP SDK 2.0.0,
`tokio-util` 0.7.17 for cancellation, and `tracing` 0.1.44 /
`tracing-subscriber` 0.3.22 for stderr-only composition diagnostics. Existing
Tokio features were expanded for bounded actors, stdio, and signals. Every
workspace path dependency is exact-version constrained.

## Protocol and runtime

The adapter pins `agent-client-protocol` 2.0.0 with schema 1.5.0 and emits wire
protocol v1. The compatibility layer adds the frozen top-level
`PromptResponse.userMessageId` while retaining official SDK parsing, dispatch,
and framing. Bounded callbacks enqueue work; tracked tasks and terminal barriers
preserve update/response order.

Initialization truthfully advertises text, image, embedded context,
cancellation, and ephemeral lifecycle support. Terminal auth method
`zai-api-key-setup` validates configured credentials without network or writes.
Unsupported capabilities are not advertised. Invalid methods and missing
sessions receive request-scoped errors without terminating the dispatcher.

The runtime registers neutral factories, rejects duplicates/unknown providers,
owns one actor per session, and uses hierarchical cancellation. Sessions are
strictly in-memory. Provider tool calls are surfaced and failed as unsupported
without execution.

Reasoning, content, usage, warnings, tool lifecycle, cancellation, and terminal
events preserve sequence. Terminal barriers preserve `userMessageId` response
placement after all prior updates. Twelve process suites validate real stdio,
loopback GLM, malformed-input containment, tool non-execution, and
post-cancellation silence.

Stage 4.1 adds bounded ACP output flow control at the official SDK stdio
boundary. The SDK writer may use an internal unbounded queue, so the adapter
waits until each `session/update` reaches the physical writer before accepting
another runtime update. This bounds outstanding visible updates and propagates
slow-reader pressure to the runtime and provider without dropping deltas.

## Compatibility and intentional differences

- Cross-process load/resume and legacy disk replay are deferred to Stage 5.
- Full slash commands, tools, permissions, and agent behavior are not
  advertised or faked.
- ACP SDK types stay in `vesper-acp`; GLM is wired only in the binary.
- Fixture payloads are unchanged: 76 scenarios, 154 payloads,
  `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.

## Verification

The seven mandatory process interleavings now pass locally. The only readiness
qualification is unexecuted remote target-family CI.

## Final gate results

- Workspace tests: 105 passed; zero failed or ignored.
- Rust 1.88: 105 passed.
- Formatting/Clippy: passed with warnings denied.
- Architecture: passed for 11 packages.
- Cargo Audit/Deny: passed; reviewed transitive duplicate warnings remain.
- Fixtures: 76 scenarios and 154 hashes validated; index unchanged.
- Process transcripts: 12 passed (seven vectors plus pressure cancellation and
  the prior four).
- Source invariance: frozen HEAD, zero tracked changes, only the pre-existing
  roadmap file untracked.
- Remote five-target CI: PENDING

## Remaining risks

Remote Linux ARM64, macOS Intel, macOS Apple Silicon, and Windows x86-64 jobs
have not executed. The portable process vectors are prepared in the five-target
workflow; Linux RSS evidence is local-only.

## Target Git state

The repository remains on `main` with no initial commit and no remote. Git
reports 21 top-level untracked paths containing the pre-existing Stages 0–3 and
the new Stage 4 work; no commit was created.

READY FOR STAGE 5 — SESSION PERSISTENCE READ PATH AND REPLAY WITH CI VALIDATION PENDING
