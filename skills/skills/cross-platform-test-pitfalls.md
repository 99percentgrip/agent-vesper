---
name: cross-platform-test-pitfalls
description: Cross-platform test pitfalls in the Agent Vesper workspace — write tests that stay green on the five-target CI matrix (Linux x86/arm, macOS intel/apple-silicon, Windows), and diagnose failures from real CI logs.
version: 1.2.0
author: Agent Vesper library (migrated from legacy GLM-ACP)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [ci, cross-platform, testing, verification]
prerequisites:
  commands: [cargo, gh]
---

# Cross Platform Test Pitfalls

CI runs `cargo test --workspace --all-features` on 5 targets (linux x86/arm,
macos intel/apple-silicon, windows). A test green on Linux but red on
macOS/Windows is usually one of these — fix proactively, don't wait for CI:

1. CANONICALIZE BOTH SIDES OF A PATH COMPARISON. `Path::canonicalize()`
   changes the path: on macOS tempdirs live behind `/var` -> `/private/var`
   (a symlink), and on Windows it adds a `\\?\` prefix. So
   `canonicalized.starts_with(tempdir.path())` is FALSE on macOS/Windows
   even when correct. When a test asserts a canonicalized path is inside a
   tempdir root, compare against `tempdir.path().canonicalize()` (the same
   reference `confine`/canonicalize uses).

2. `sleep` IS UNIX-ONLY. Windows `cmd` has no `sleep`. For a test that
   needs a long-running command under a timeout, branch with cfg:
   `#[cfg(unix)] let cmd = "sleep 5"; #[cfg(windows)] let cmd = "ping -n 6
   127.0.0.1";`. (`timeout /t` fails under piped stdin; `ping -n N` is
   reliable.)

3. SHELL COMMANDS VARY. `echo` works in both `sh -c` and `cmd /C`; most
   others don't. Keep run_command tests to commands that exist on both, or
   cfg-branch.

4. DIAGNOSE FROM REAL LOGS, DON'T GUESS. When the platform matrix fails on
   macOS/Windows: `gh run list --workflow platform-foundation.yml --limit 1`
   to get the run id, then `gh run view <id> --log-failed` to pull the
   exact panicked assertions. The real log pinpoints the failing line in
   seconds; guessing CRLF (the usual suspect) can waste hours when macOS is
   failing too.

5. CONFIRM, DON'T ASSUME. After the fix: `gh run watch <id> --exit-status`
   to wait for green on all 5 targets before reporting done.

## Provenance

Learned in the legacy GLM-ACP agent on 2026-08-01; migrated to the Agent
Vesper library.
