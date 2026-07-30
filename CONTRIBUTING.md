# Contributing

Agent Vesper is being migrated in bounded, fixture-gated stages. Read the
repository `AGENTS.md`, the applicable child instructions, the accepted
[ADRs](docs/adr/), and [migration status](docs/migration-status.md) before
editing.

## Change boundaries

- Keep the Native GLM ACP source repository read-only.
- Do not add a future production crate before its owning migration stage.
- Production crates must never depend on `vesper-testkit`, frontend crates, or
  disposable spikes.
- Provider adapters must not depend on the future core engine.
- Do not regenerate authoritative fixtures unless a schema defect is proven and
  reviewed.
- Every direct dependency needs an entry in [docs/dependencies.md](docs/dependencies.md).
- Unsafe code is denied by each current crate. Future platform bindings require
  an explicit isolated module, a safety comment for each block, and review.

## Required checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --doc
cargo xtask fixtures validate
cargo xtask fixtures verify-index
cargo xtask architecture
cargo xtask verify
```

Run `cargo xtask msrv` when Rust 1.88.0 is installed. CI owns the five-target
matrix; cross-compilation alone is not runtime validation.

## Pull requests

Describe contract changes, fixture coverage, security consequences, platform
status, and deferred work. Never claim an unexecuted platform test passed or an
unimplemented feature exists.
