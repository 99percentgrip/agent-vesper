# Production architecture decisions

## Purpose

Own accepted, durable architecture and compatibility decisions for Agent Vesper.

## Local Contracts

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

## Verification

- Run `cargo xtask architecture`.
- Check every accepted ADR contains compatibility, security, migration, and
  verification consequences.

## Child DOX Index

No children.
