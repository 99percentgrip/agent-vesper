---
name: verify-with-xtask-verify
description: Correctly verify changes in the Agent Vesper Rust workspace so CI passes on ALL five targets and the verification guard recognizes success — cargo fmt, xtask verify locally, then the five-target platform matrix before reporting done.
version: 1.2.0
author: Agent Vesper library (migrated from legacy GLM-ACP)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [verify, ci, cross-platform, rust]
prerequisites:
  commands: [cargo, gh]
---

# Verify With Xtask Verify

The "done" gate has TWO layers — local AND the GitHub five-target CI
matrix. Skipping either causes failures the user has to report.

LAYER 1 — LOCAL (Linux host only): `cargo run --package xtask --quiet --
verify` runs fmt + clippy + tests + architecture + the ACP stdout-purity
gate. Run `cargo fmt` then `cargo fmt --check` BEFORE committing. This is
NECESSARY but NOT SUFFICIENT — it exercises only the local host.

LAYER 2 — CI MATRIX (the real cross-platform gate): `cargo xtask verify`
does NOT run on macOS/Windows. After pushing changes that touch paths,
files, canonicalize, shell commands, or raw bytes, the "done" gate MUST
include:

    gh run list --workflow platform-foundation.yml --limit 1   # get the run id
    gh run watch <id> --exit-status                            # wait for ALL 5 targets green

Only when linux-x86_64, linux-arm64, macos-intel, macos-apple-silicon,
windows-x86_64 are ALL green is the work "verified." Do NOT report done on
local-Linux-green alone for cross-platform-sensitive code.

Workflow: Implement → `cargo fmt` → `cargo fmt --check` →
`cargo xtask verify` (local) → fix gaps, rerun → update AGENTS.md/README →
commit (stage explicit paths; exclude agent state dirs `.agent-vesper/`
and `.glm-acp/`) → push → `gh run watch <id> --exit-status` (confirm 5
targets) → only THEN report done. Never weaken tests to pass.

Pitfalls that have caused real CI failures the user had to report:

1. FMT GATE: `cargo fmt --check` is part of xtask verify and is easy to
   miss. Multi-line `use {...}` that fits one line, long
   signatures/asserts.
2. STDOUT-PURITY: `apps/agent-vesper-acp/src/main.rs` must contain NO
   `println!` (xtask flags it). Route non-protocol output to `eprintln!`.
3. NEW CRATE → ALLOWED DEPS: register the crate in
   `xtask/src/main.rs::allowed_dependencies()` (incl. `vesper-testkit` if
   dev-dep) or the architecture gate fails.
4. MSRV CLIPPY: MSRV is 1.88; avoid std APIs stabilized after.
5. VISIBLE OUTPUT + NO BASHISMS: the verification guard recognizes success
   only from normal (non-piped, non-redirected) output. `run_command` runs
   `/bin/sh` — no `${PIPESTATUS[@]}`/`[[ ]]`; capture exit via
   `; echo "exit=$?"`.

## Provenance

Learned in the legacy GLM-ACP agent on 2026-08-01 (rev 3); migrated to the
Agent Vesper library.
