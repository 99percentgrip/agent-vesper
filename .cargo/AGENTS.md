# Cargo command policy

## Purpose

Own repository-local Cargo aliases and dependency-resolution behavior.

## Local Contracts

- `cargo xtask` is the supported entry point for repository maintenance.
- Dependency resolution must respect workspace `rust-version = "1.88"`.
- Do not add target-specific runtime behavior here.
- `config.toml` may declare target-scoped build settings only. The current
  contents switch the `x86_64-pc-windows-msvc` linker to the self-contained
  `rust-lld` (`lld-link` flavor) so the Windows CI matrix links 3–5× faster
  than MSVC `link.exe`. Target-scoped config never affects Linux/macOS
  builds.

## Verification

- Run `cargo xtask architecture`.
- Run the dedicated Rust 1.88 verification command.

## Child DOX Index

No children.
