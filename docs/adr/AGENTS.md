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

## Verification

- Run `cargo xtask architecture`.
- Check every accepted ADR contains compatibility, security, migration, and
  verification consequences.

## Child DOX Index

No children.
