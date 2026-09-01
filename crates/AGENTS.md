# Production crates

## Purpose

Own provider-neutral foundations, the GLM leaf adapter, Stage 4 runtime/ACP
boundaries, and Stage 5 read-only session discovery, conversion, replay, and
test-only conformance support.

## Local Contracts

- `vesper-domain` depends on no workspace crate.
- `vesper-provider` depends only on `vesper-domain`.
- `vesper-security` depends on no workspace crate.
- `vesper-auth` depends only on `vesper-security`; it owns native OS
  credential-manager access and the strict owner-only Unix vault fallback
  (ADR 0014 — Agent Vesper Authentication).
- `vesper-memory` depends only on `vesper-domain` and `vesper-security`; it
  owns the durable memory graph, learned skills, user profile, and bounded
  awareness ledger (ADR 0011 — Stage 12).
- `vesper-cognition` depends only on `vesper-domain` and `vesper-security`
  (plus `rusqlite` (bundled), `rust-stemmers`, and standard utility crates);
  it owns the memory-oracle-equivalent V3 cognitive memory engine — single-pass
  ADD-only extraction, hybrid semantic + FTS5 BM25 + entity-boost retrieval,
  and the embedded SQLite backing (ADR 0015 — Stage 16). It is the **only**
  production crate permitted to declare `rusqlite`; provider embeddings +
  extraction LLM + entity NLP are trait ports fulfilled at the composition
  boundary, never inside this crate.
- `vesper-checkpoints` depends only on `vesper-domain` and
  `vesper-security`; it owns the workspace snapshot/rollback, session
  lineage, and bounded cron/export/clipboard/CI surface (ADR 0012 —
  Stage 14). Strict RAII (`Drop`) file-handle discipline — no SQLite, no
  git refs, no auto-snapshotting.
- `vesper-mcp` depends only on `vesper-domain` and `vesper-security`
  (plus `ed25519-dalek`); it owns the MCP stdio client and the
  Ed25519-signed plugin loader (ADR 0013 — Stage 15). The unsigned-plugin
  loading code path is structurally erased from `--release` builds via
  `#[cfg(debug_assertions)]`.
- `vesper-config` depends only on `vesper-domain` and `vesper-security`.
- `vesper-policy` depends only on `vesper-domain` and `vesper-security`.
- `vesper-sandbox` depends only on `vesper-security` plus platform `libc`
  confined to the Linux-only `sandbox_init` supervisor binary target; it owns
  the opt-in namespaces backend with probed, honest capabilities (ADR 0022 —
  Sandbox Supervisor as the Sole Raw-Syscall Boundary).
- `vesper-testkit` may depend on all foundational crates and owns synthetic
  read-store/no-write helpers; no production crate may depend on it.
- `vesper-provider-glm` may depend on auth/domain/provider/config/security and
  use `vesper-testkit` only as a dev dependency.
- `vesper-runtime` may depend on domain/provider and the read-only repository,
  converted-state, and transactional write ports from `vesper-sessions`;
  filesystem I/O remains implemented only by `vesper-sessions`, and runtime
  remains independent of ACP and concrete providers.
- `vesper-acp` may depend on domain/runtime; it maps read-only persistent
  lifecycle outcomes without directly accessing storage, and all official ACP
  SDK types stay in this crate.
- `vesper-sessions` may depend on domain/config and owns read-only, bounded
  discovery, legacy decoding, safe metadata, pure runtime-state seeds,
  deterministic identities, ACP-neutral replay plans, bounded persisted
  search, and the Stage 6 transactional Agent Vesper session writer. It must
  not depend on runtime,
  ACP, GLM, SQLite, or testkit in production.
- HTTP and concrete GLM behavior are confined to `vesper-provider-glm`; no crate
  may depend on ACP, SQLite, TUI, MCP, or a disposable spike.
- Unsafe code is denied by the current crates. Future platform exceptions
  require a dedicated module, safety comments, review, and ADR update. ADR 0022
  grants exactly one standing exception: `vesper-sandbox`'s `sandbox_init`
  supervisor binary (raw syscalls for namespaces/mounts/`fork`/`execve`,
  `#![deny(unsafe_op_in_unsafe_fn)]` + documented-block discipline, enforced by
  `cargo xtask architecture`); the `vesper-sandbox` library itself stays
  100% safe code.

## Verification

- Run `cargo xtask architecture`.
- Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Run `cargo test --workspace --all-features`.

## Child DOX Index

- `vesper-domain/AGENTS.md` — stable provider-neutral values and events.
- `vesper-config/AGENTS.md` — platform paths, profiles, and typed configuration.
- `vesper-provider/AGENTS.md` — provider ports, capabilities, and stream rules.
- `vesper-security/AGENTS.md` — secret-safe and authority-boundary primitives.
- `vesper-auth/AGENTS.md` — native-first provider-neutral credential storage
  and owner-only Unix vault fallback.
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
- `vesper-cognition/AGENTS.md` — ADR 0015 (Stage 16) memory-oracle-equivalent V3
- `vesper-checkpoints/AGENTS.md` — ADR 0012 (Stage 14) workspace snapshots,
  rollback, session lineage, and bounded cron/export/clipboard/CI surface.
- `vesper-mcp/AGENTS.md` — ADR 0013 (Stage 15) MCP stdio client and
  Ed25519-signed plugin loader with `#[cfg(debug_assertions)]` dev mode.
- `vesper-agent/AGENTS.md` — Tier C (ADR 0010). The multi-turn
  tool-executing agent loop, tool registry + executors, and permission gating
  that compose `vesper-runtime`. Owns no provider-wire, ACP mapping, or
  persistence internals.
- `vesper-harness/AGENTS.md` — shared hosted Python-oracle tool services for
  ACP and TUI compositions.
- `vesper-observability/AGENTS.md` — opt-in secret-safe trajectory recording
  and bounded reliability aggregation for composed hosts.
