# Secure credential storage

## Purpose

Own provider-neutral credential persistence through operating-system credential
managers with an explicit owner-only file fallback on Unix systems.

## Ownership

- `src/lib.rs` owns credential identifiers, validation, native-store access,
  strict private-file persistence, secret-free Windows cross-process leases,
  and secret-safe errors/receipts.
- `src/native_records.rs` owns Windows size-bounded native records, immutable
  generations, readback/integrity, interruption recovery and sign-out tombstones.
- `tests/native_persistence.rs` owns explicit hosted synthetic save/restart/
  rotation/sign-out acceptance through the public production store.

## Local Contracts

- Depend only on `vesper-security` among workspace crates.
- Never include credential values in formatting, errors, logs, or serialized
  metadata other than the explicitly authorized private fallback vault.
- Prefer the native OS credential manager. A fallback vault must be created
  atomically with directory mode `0700` and file mode `0600` on Unix.
- A successful native write removes that credential's older fallback. A
  retained fallback is authoritative on reads, preventing an older keyring
  value from resurfacing after a newer fallback rotation or logout.
- Fail closed instead of creating a permission-unverified fallback on Windows.
- Ordinary tests use path-explicit private stores or the Windows-sized in-memory
  backend and never access user OS keyrings. ADR 0032 permits explicitly opted-in
  hosted acceptance using only UUID namespaced synthetic entries, fresh-process
  reload and cleanup on disposable GitHub runners.
- Windows records exceed the native blob limit only through bounded immutable
  chunks, verified primary descriptors and a native transaction journal. Never
  add a plaintext Windows fallback. The secret-free file lease is bounded and
  every process for one native identity must use the same configured vault path.
- Existing explicit vault reads keep retained-value precedence. Windows must
  refuse a write/delete before native mutation when a retained vault credential
  cannot be safely retired; startup and private signed-out fixtures remain read-only.
- Composite records are bounded to 256 KiB; serialized Unix vaults to 1 MiB.
  Oversized writes must preserve the previous readable vault.

## Work Guidance

- Keep provider identity data-driven; this crate does not register providers.
- Preserve bounded inputs and atomic replacement for every fallback write.

## Verification

- Run `cargo test -p vesper-auth --all-features`.
- Run `cargo xtask architecture` and strict workspace Clippy.

## Child DOX Index

No children.
