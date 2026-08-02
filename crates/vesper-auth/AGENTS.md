# Secure credential storage

## Purpose

Own provider-neutral credential persistence through operating-system credential
managers with an explicit owner-only file fallback on Unix systems.

## Ownership

- `src/lib.rs` owns credential identifiers, validation, native-store access,
  strict private-file persistence, and secret-safe errors/receipts.

## Local Contracts

- Depend only on `vesper-security` among workspace crates.
- Never include credential values in formatting, errors, logs, or serialized
  metadata other than the explicitly authorized private fallback vault.
- Prefer the native OS credential manager. A fallback vault must be created
  atomically with directory mode `0700` and file mode `0600` on Unix.
- Fail closed instead of creating a permission-unverified fallback on Windows.
- Tests use path-explicit private stores and never access live OS keyrings.

## Work Guidance

- Keep provider identity data-driven; this crate does not register providers.
- Preserve bounded inputs and atomic replacement for every fallback write.

## Verification

- Run `cargo test -p vesper-auth --all-features`.
- Run `cargo xtask architecture` and strict workspace Clippy.

## Child DOX Index

No children.
