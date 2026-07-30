# Migration validation workflows

## Purpose

Own CI definitions used to validate production migration gates and disposable
platform assumptions on hosts unavailable locally.

## Local Contracts

- Workflows must not call live model providers or require provider credentials.
- Keep the five release-target families explicit in the matrix: linux-x86_64,
  linux-arm64, macos-intel, macos-apple-silicon, windows-x86_64.
- Do not add publishing, release, push, or deployment steps.
- The default toolchain is pinned to Rust 1.95.0 via `rust-toolchain.toml`
  (with `clippy` and `rustfmt` components); MSRV 1.88.0 is enforced
  independently in `msrv.yml` and the spike workflows.
- `ci.yml` runs the canonical verification gate via `cargo xtask verify`
  (fmt, clippy `-D warnings`, architecture, fixtures per stage, contract
  conformance, GLM/runtime/ACP/sessions verify, and the full workspace test
  suite) plus a documentation-structure check.
- The supply-chain job runs `cargo audit` and `cargo deny --all-features check`
  with pinned tool versions (`cargo-audit 0.22.2`, `cargo-deny 0.20.2`).
- `deny.toml` enforces allowed licenses, bans SQLite crates and wildcard
  dependencies, and treats duplicate versions as warnings (the documented
  Stage 5 baseline) rather than failures.
- The five-target matrix runs the Stage 4.1 real-process blocker suite with
  bounded timeouts; Linux-only RSS evidence must not be generalized.
- CI validates Stage 5 coverage, read-only session/testkit conformance, and
  writer/SQLite architecture gates on all five target families.

## Verification

- Validate YAML syntax locally where tooling exists.
- Record actual run URLs and outcomes in foundation reports before upgrading a
  platform result from pending. The pipeline configuration is complete and the
  canonical gate passes locally; workflow-run outcomes remain pending until a
  real job executes against a configured remote.

## Child DOX Index

- `workflows/ci.yml` — pull-request canonical gate (`cargo xtask verify`),
  documentation-structure check, and the supply-chain job (`cargo audit` +
  `cargo deny check`).
- `workflows/msrv.yml` — dedicated Rust 1.88.0 foundational verification with
  per-stage fixture coverage.
- `workflows/platform-foundation.yml` — five-target production-foundation and
  eligible spike matrix.
- `workflows/foundation-spikes.yml` — five-target disposable spike test matrix.
