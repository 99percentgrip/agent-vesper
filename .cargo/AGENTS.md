# Cargo command policy

## Purpose

Own repository-local Cargo aliases and dependency-resolution behavior.

## Local Contracts

- `cargo xtask` is the supported entry point for repository maintenance.
- Dependency resolution must respect workspace `rust-version = "1.88"`.
- Do not add target-specific runtime behavior here.

## Verification

- Run `cargo xtask architecture`.
- Run the dedicated Rust 1.88 verification command.

## Child DOX Index

No children.
