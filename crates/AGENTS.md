# Production crates

## Purpose

Own provider-neutral foundations, the GLM leaf adapter, Stage 4 runtime/ACP
boundaries, and Stage 5 read-only session discovery, conversion, replay, and
test-only conformance support.

## Local Contracts

- `vesper-domain` depends on no workspace crate.
- `vesper-provider` depends only on `vesper-domain`.
- `vesper-security` depends on no workspace crate.
- `vesper-memory` depends only on `vesper-domain` and `vesper-security`; it
  owns the durable memory graph, learned skills, user profile, and bounded
  awareness ledger (ADR 0011 — Stage 12).
- `vesper-checkpoints` depends only on `vesper-domain` and
  `vesper-security`; it owns the workspace snapshot/rollback, session
  lineage, and bounded cron/export/clipboard/CI surface (ADR 0012 —
  Stage 14). Strict RAII (`Drop`) file-handle discipline — no SQLite, no
  git refs, no auto-snapshotting.
- `vesper-config` depends only on `vesper-domain` and `vesper-security`.
- `vesper-policy` depends only on `vesper-domain` and `vesper-security`.
- `vesper-testkit` may depend on all foundational crates and owns synthetic
  read-store/no-write helpers; no production crate may depend on it.
- `vesper-provider-glm` may depend on domain/provider/config/security and use
  `vesper-testkit` only as a dev dependency.
- `vesper-runtime` may depend on domain/provider and the read-only repository,
  converted-state, and transactional write ports from `vesper-sessions`;
  filesystem I/O remains implemented only by `vesper-sessions`, and runtime
  remains independent of ACP and concrete providers.
- `vesper-acp` may depend on domain/runtime; it maps read-only persistent
  lifecycle outcomes without directly accessing storage, and all official ACP
  SDK types stay in this crate.
- `vesper-sessions` may depend on domain/config and owns read-only, bounded
  discovery, legacy decoding, safe metadata, pure runtime-state seeds,
  deterministic identities, ACP-neutral replay plans, and the Stage 6
  transactional Agent Vesper session writer. It must not depend on runtime,
  ACP, GLM, SQLite, or testkit in production.
- HTTP and concrete GLM behavior are confined to `vesper-provider-glm`; no crate
  may depend on ACP, SQLite, TUI, MCP, or a disposable spike.
- Unsafe code is denied by the current crates. Future platform exceptions
  require a dedicated module, safety comments, review, and ADR update.

## Verification

- Run `cargo xtask architecture`.
- Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Run `cargo test --workspace --all-features`.

## Child DOX Index

- `vesper-domain/AGENTS.md` — stable provider-neutral values and events.
- `vesper-config/AGENTS.md` — platform paths, profiles, and typed configuration.
- `vesper-provider/AGENTS.md` — provider ports, capabilities, and stream rules.
- `vesper-security/AGENTS.md` — secret-safe and authority-boundary primitives.
- `vesper-policy/AGENTS.md` — pure permission and policy decisions.
- `vesper-testkit/AGENTS.md` — fixture and fake-conformance helpers.
- `vesper-provider-glm/AGENTS.md` — Z.ai GLM provider adapter.
- `vesper-provider-synthetic/AGENTS.md` — deterministic in-process reference
  provider proving multi-provider contract neutrality.
- `vesper-runtime/AGENTS.md` — provider-neutral session actors and converted
  state acceptance. (Single-turn engine; composed — not modified — by
  `vesper-agent` under ADR 0010.)
- `vesper-acp/AGENTS.md` — official-SDK ACP protocol-v1 adapter.
- `vesper-sessions/AGENTS.md` — read-only session ports, bounded compatibility
  decoding, conversion, identity, replay plans, layouts, metadata, and the
  Stage 6 transactional writer.
- `vesper-memory/AGENTS.md` — ADR 0011 (Stage 12) persistent memory graph,
  learned skills, user profile, and bounded epistemic ledger.
- `vesper-checkpoints/AGENTS.md` — ADR 0012 (Stage 14) workspace snapshots,
  rollback, session lineage, and bounded cron/export/clipboard/CI surface.
- `vesper-agent/AGENTS.md` — Tier C (ADR 0010). The multi-turn
  tool-executing agent loop, tool registry + executors, and permission gating
  that compose `vesper-runtime`. Owns no provider-wire, ACP mapping, or
  persistence internals.
