# Fixture and conformance testkit

## Purpose

Own Rust consumers for the language-neutral fixture corpus, deterministic
fakes, and synthetic read-store/disk-invariance helpers.

## Local Contracts

- This crate is non-production and may depend on foundational crates.
- Production crates must never depend on this crate.
- Fixture data is loaded from `fixtures/`; do not duplicate captured payloads.
- Temporary stores may write only beneath unique test-owned temporary roots and
  must never discover or mutate real user state.
- Hash/no-write manifests record the complete synthetic tree without following
  symlinks.
- Normalization never changes event order, linkage, policy outcomes, finish
  reasons, hashes, redaction, or cancellation classification.

## Verification

- Run `cargo test -p vesper-testkit`.
- Run `cargo xtask fixtures validate`.
- Run `cargo xtask fixtures verify-index`.
- Run `cargo xtask fixtures coverage --stage 5`.
- Run `cargo xtask sessions verify`.
- Run `cargo xtask contracts verify`.

## Child DOX Index

No children.
