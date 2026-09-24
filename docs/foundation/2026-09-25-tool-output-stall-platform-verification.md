# Tool-Output Stall Platform Verification Preparation — 2026-09-25

## Objective

Close the release-relevant Windows/macOS behavioral gap for the accepted
classification-D shared executor repair without committing, pushing, versioning,
tagging, releasing or installing.

## Method and Findings

The five-target workflow accepts only a GitHub ref and checks out repository
content. GitHub-hosted Windows 2025, macOS 15 Intel and macOS 15 Apple-silicon
runners therefore cannot execute this uncommitted worktree. Dispatching `main`
would test baseline `01461a32…`, not this repair; creating a remote ref would
violate Alex's explicit no-commit/no-push constraint. No misleading workflow was
launched.

The audit found two concrete platform defects before remote execution:

- `command_settlement.rs` was globally `cfg(unix)`, so Windows executed zero new
  behavior tests. Its descendant-absence assertions were Linux-only.
- Windows cleanup used `taskkill /T` after the shell leader could already have
  exited. That PID-tree rediscovery cannot reliably own an orphaned former child.

The candidate now uses `command-group` 5.0.1: an owned POSIX process group on
Unix and a retained Windows Job Object with kill-on-close. The portable test
matrix uses Windows PowerShell/cmd fixtures or POSIX fixtures and delayed marker
files to prove descendant cleanup after timeout, signal cancellation, caller
abort, leader exit and continued descendant writing. The five-target workflow
has a named, bounded command-settlement step, preventing empty filtered tests.

## Evidence

- Red during local abstraction integration: 6/9 Linux cases reported cleanup
  uncertainty because POSIX `ESRCH` (the owned group is already absent) was not
  classified as successful absence.
- Green after exact correction: 9/9 portable settlement cases passed on Linux
  in 14.05 seconds, then 9/9 passed in the full agent suite in 3.55 seconds.
- `cargo test -p vesper-agent`: PASS (425 unit tests plus integration suites).
- ACP hosted registry route: PASS 1/1.
- TUI hosted registry route: PASS 1/1 after moving generated artifacts out of
  the bounded `/tmp` filesystem.
- `cargo +1.88.0 check -p vesper-agent --all-targets`: PASS.
- strict affected-package Clippy: PASS.
- `cargo xtask architecture`: PASS, 30 packages.
- `cargo deny check`: PASS, including the new dependency.
- Linux-to-Windows/macOS test cross-build attempts were blocked by missing native
  `aws-lc-sys` target C toolchains. They are not counted as platform evidence.

## Acceptance Interpretation

The authoritative incident record explicitly labels matched live OpenAI/GLM
comparison as patch-acceptance item 6. It therefore remains OPEN / NOT EXECUTED
pending Alex authorization. This is an incident-closeout evidence requirement,
not missing proof for classification D: both adapters have fixture-backed tool
surface/protocol parity, and the provider-neutral production executor reproduced
the pipe defect directly.

Installed/source identity is a historical provenance limitation. The old
installed 0.23.3 incident binary is not byte-identical to source baseline 0.23.5,
but current source independently reproduced the same mechanism red at the real
`RunCommand` boundary. Release gating can therefore evaluate the repaired current
source without establishing the old binary's unavailable digest-to-source map.

## Changed Files

- `Cargo.toml`, `Cargo.lock`, `crates/vesper-agent/Cargo.toml`
- `crates/vesper-agent/src/tools.rs`
- `crates/vesper-agent/tests/command_settlement.rs`
- `.github/workflows/platform-foundation.yml`
- applicable `.github`, crate and foundation DOX records
- the incident and permanent-repair reports

## Deviations and Unresolved Items

- Windows 2025 behavior: NOT RUN.
- macOS 15 Intel behavior: NOT RUN.
- macOS 15 Apple-silicon behavior: NOT RUN.
- Live OpenAI/GLM correlation: NOT EXECUTED pending explicit authorization.
- The prepared candidate requires a committed remote ref before GitHub-hosted
  runners can execute it. No commit or push was performed.

## Readiness Effect

The implementation and CI matrix are prepared for an exact-ref run. Alex
authorized one dedicated temporary verification commit/ref on 2026-09-25; hosted
behavioral results remain pending until that workflow completes.
