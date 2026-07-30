# Disposable Rust Spikes

## Purpose

Own bounded technical experiments that validate migration assumptions without
starting production Agent Vesper implementation.

## Ownership

- Each child directory is an independent disposable Cargo package and records
  its own tested assumption.

## Local Contracts

- No spike is a production crate or public Vesper API.
- Pin exact dependency versions and commit `Cargo.lock`.
- Use fixture-driven/local-only behavior and never live providers.
- Platform claims are limited to hosts/CI jobs actually executed.

## Work Guidance

- Keep experiments small enough to delete or rewrite after ADR review.

## Verification

- Run each spike's documented `cargo test --locked`.

## Child DOX Index

- `acp-rust-protocol-v1/AGENTS.md` — official ACP SDK/wire-v1 compatibility.
- `rust-sse-transport/AGENTS.md` — bounded HTTP/SSE/cancellation semantics.
- `sqlite-fts5/AGENTS.md` — bundled/system SQLite FTS5 packaging and rebuild behavior.
- `process-sandbox/AGENTS.md` — process-tree cleanup and platform sandbox conformance.
