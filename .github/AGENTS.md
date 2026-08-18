# Migration validation workflows

## Purpose

Own CI definitions used to validate production migration gates and disposable
platform assumptions on hosts unavailable locally.

## Local Contracts

- Keep the five release-target families explicit in the matrix: linux-x86_64,
  linux-arm64, macos-intel, macos-apple-silicon, windows-x86_64.
- Validation workflows must not call live providers or require credentials.
  The tag-triggered `release.yml` workflow is the sole publishing workflow and
  may publish only compiled, checksummed release archives; it must not make
  provider calls.
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
- `release.yml` declares a single concurrency group `release-pipeline` with
  `cancel-in-progress: true` so a new tag-push (or `workflow_dispatch` with
  a `tag` input) cancels any stuck prior run (e.g. a phantom-queued run
  left behind by a runner-image `Service Unavailable` failure). Without
  this guard, GitHub Actions blocks all subsequent release runs behind the
  stuck one indefinitely. `workflow_dispatch` is the manual recovery path
  when a tag is already pushed but no run fired: `gh workflow run
  release.yml --ref <tag> -f tag=<tag>`.

## Verification

- Validate YAML syntax locally where tooling exists.
- The canonical gate (`cargo xtask verify`) and MSRV 1.88 verification pass on
  every push. Linux (x86_64, arm64) and macOS (intel, apple-silicon) targets
  are verified green on the five-target matrix. Windows build-time is being
  optimized via Cargo artifact caching.

## Child DOX Index

- `workflows/ci.yml` — pull-request canonical gate (`cargo xtask verify`),
  documentation-structure check, and the supply-chain job (`cargo audit` +
  `cargo deny check`).
- `workflows/msrv.yml` — dedicated Rust 1.88.0 foundational verification with
  per-stage fixture coverage.
- `workflows/platform-foundation.yml` — five-target production-foundation and
  eligible spike matrix.
- `workflows/foundation-spikes.yml` — five-target disposable spike test matrix.
- `workflows/release.yml` — tag-triggered ACP+TUI archive packaging and GitHub
  Release publication for the registry and installers; archives also bundle
  the repo `skills/` seed library seeded by the installers into
  `~/.agent-vesper/memory/`; the registry continues
  to launch only the ACP binary from the shared bundle.
