# Security primitives

## Purpose

Own secret-safe values, redaction, environment scrubbing, bounded untrusted
output, path authority descriptors, and honest sandbox capability contracts.

## Local Contracts

- This crate depends on no workspace crate and grants no filesystem/process authority.
- Secret values never expose contents through Debug, Display, errors, or serialization.
- Complete sandbox/process/filesystem enforcement belongs to later stages.
- Unsafe code is forbidden in Stage 1.

## Verification

- Run `cargo test -p vesper-security`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
