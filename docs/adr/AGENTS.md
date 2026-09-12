# Production architecture decisions

## Purpose

Own accepted, durable architecture and compatibility decisions for Agent Vesper.

## Local Contracts

- ADR 0029 refines ADR 0028 enrollment: native opt-in permits automatic PRD
  recognition with independent scope review and remembered paths; pending scope
  grants no completion and evidence requirements remain unchanged.
- ADR 0028 defines scoped native implementation acceptance, original-requirement
  coverage, collector-owned evidence, independent review, bounded repair, both-host
  publication and opt-in audit lineage. The live gate is separate from model plan status
  and provider stop.

- Accepted ADRs are immutable decisions; superseding requires a new ADR.
- Every ADR links historical foundation evidence and executable verification.
- Existing foundation ADRs remain preserved under `docs/foundation/adr/`.
- ADR 0015 authorizes the first production SQLite dependency (`vesper-cognition`).
  The blanket Stage-5 SQLite prohibition is superseded by a per-crate
  allowlist exception in `cargo xtask architecture` (only `vesper-cognition`
  may declare `rusqlite`).
- ADR 0016 introduces the provider-independent embedding layer. The active
  chat provider no longer determines the embedding source — that decision is
  owned by `.agent-vesper/cognition/embedding.json`. Cosine similarity
  cannot silently fail (Gap 10 eliminated structurally).
- ADR 0017 (VRO-11) introduces VesperLens: a native Rust loopback oracle
  for human-in-the-loop HTML artifact review. Lives under
  `crates/vesper-agent/src/planning/vesper_lens/`, is built on raw
  `tokio::net::TcpListener` (zero new external deps — only the `net` +
  `io-util` features on the existing workspace tokio pin), binds strictly
  to `127.0.0.1:0`, and defines its own minimal JSON feedback contract.
  The MIT-licensed reference Oracle repo was read as a
  user-authorized reference blueprint; no code was copied (the harness
  scanner flagged its overlay JS as prompt-injection-shaped).
- ADR 0018 keeps ADR 0017's network/security boundaries while adding the
  automatic browser handoff, interaction-first artifact review, structured
  browser planning questions/answers, and the dedicated TUI TODO panel.
- ADR 0019 replaces ADR 0018's fixed four-question host cap with a typed
  session policy: fixed 1–12 or agent-selected auto 1–12, defaulting to four,
  with matching per-turn tool schema and executor enforcement.
- ADR 0020 supersedes ADR 0017/0018's same-document, unauthenticated,
  single-turn review boundary with trusted outer chrome, a sandboxed artifact,
  authenticated session routes, confined sibling assets, reusable in-process
  sessions, precise annotation targets, richer interviews, conditional HTML
  review triggering, and real-browser verification. ADR 0019 remains active.
- ADR 0021 composes independent project and global cognitive-memory stores in
  the TUI, with conservative smart routing, explicit overrides, visible scope
  confirmation, lifecycle moves, and a two-scope audit surface.
- ADR 0022 (VRO-13 PR-3) introduces `vesper-sandbox`: the library is 100% safe
  code; all namespace/mount/fork/`execve` raw syscalls live only in the
  `sandbox_init` supervisor binary, and every capability is probed and
  reported honestly, never assumed.
- ADR 0023 defines provider-neutral semantic context compaction shared by both
  hosts and all direct/VRO execution paths, including transactional history
  replacement, token pressure, auxiliary fallback, security, persistence,
  and quality lineage.
- ADR 0024 defines provider-neutral automatic and explicit skill routing:
  deterministic metadata ranking, fail-closed eligibility, bounded
  composition, transient inline loading, isolated-worker loading, cross-host
  parity, compaction-safe identity audit, and bounded outcome feedback.
- ADR 0025 preserves the original provider-neutral swarm decision. ADR 0026
  supersedes its single-stream adapter, TUI-only exclusion and completion claims:
  approved repairs require native AgentLoop/permission/tool composition, immutable
  ledger readers, both-host Settings/command parity and actual cleanup acceptance.
  Activation remains default-off. Current F01–F18 evidence lives in
  `../foundation/vro15-repair-execution.md`, not historical ADR test counts.
  Dependency/clock goals and upstream exclusions remain in force; the naming
  embargo is enforced by `cargo xtask naming-guard`.

- ADR 0027 refines ADR 0022 with private-root namespace confinement, dropped
  capabilities, correct outer IDs and bounded observed cleanup. The sole raw-
  syscall boundary and supervisor wire protocol remain unchanged.

## Verification

- Run `cargo xtask architecture`.
- Check every accepted ADR contains compatibility, security, migration, and
  verification consequences.

## Child DOX Index

No children.
