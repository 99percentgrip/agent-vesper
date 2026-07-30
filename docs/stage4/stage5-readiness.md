# Stage 5 readiness

Status: COMPLETE

## Bounded next target

Stage 5 is: persistent session read path, legacy schema-v1 loading, replay, and
Agent Vesper session-store foundation.

Inputs now available:

- stable domain/session-v1 compatibility DTOs;
- ephemeral actor snapshots and replay behavior;
- ACP in-process load/resume/fork/close mappings;
- explicit Agent Vesper/legacy state-root ADRs;
- fixture-index and compatibility conformance machinery.

Stage 5 should add read-only legacy discovery first, corruption-safe decoding,
replay parity, and an independent Vesper store contract before enabling writes.
It must not expand into tools, compaction, memory, or the full agent loop.

All seven Stage 4.1 process vectors now pass locally through the production
path. Stage 5 is locally unblocked. Linux ARM64, macOS Intel, macOS Apple
Silicon, and Windows x86-64 execution remains CI-validation pending, so the
readiness classification retains that qualification.

READY FOR STAGE 5 — SESSION PERSISTENCE READ PATH AND REPLAY WITH CI VALIDATION PENDING
