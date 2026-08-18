---
name: rust-workspace-disk-hygiene
description: Diagnose ENOSPC masquerading as toolchain crashes in the Rust workspace — rustc SIGSEGV "failed to parse process output (signal 11)" and linker "collect2: ld terminated with signal 7 [Bus error]" mean the disk is full. Check df -h FIRST; cargo clean is always safe.
version: 1.0.0
author: Agent Vesper library
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [disk, enospc, cargo-clean, toolchain, diagnostics]
---

# Rust Workspace Disk Hygiene

`target/` in a large Cargo workspace balloons silently (50 GB+ observed in
Agent Vesper after repeated release builds). When the disk fills, failures
look like CODE BUGS but are ENOSPC:

## Symptom signatures (all mean "check disk FIRST")

1. rustc crashes during a normal check/build:
   `failed to parse process output: ... rustc ... (signal: 11, SIGSEGV:
   invalid memory reference)` — often followed by `No space left on
   device (os error 28)` in the same output.
2. The linker dies mid-link:
   `collect2: fatal error: ld terminated with signal 7 [Bus error]`.
3. cargo itself reports `No space left on device` only sometimes — the
   misleading SIGSEGV/Bus-error forms can appear alone.

## Procedure

1. On ANY of those signatures, run `df -h /` (and the workspace drive)
   BEFORE reading a single line of code as the cause. 100% usage confirms
   it.
2. Check the scale: `du -sh target`.
3. Free space: `cargo clean` — this is ALWAYS safe in this repo: CI
   rebuilds its own clean state, and local rebuilds are only slow once
   (a full verify rebuild takes minutes, not hours).
4. Re-run the interrupted gate from scratch (`cargo xtask verify` or the
   specific `cargo test -p <crate>`) — partial artifacts from the
   ENOSPC-failed build must not be trusted.
5. Prevent recurrence on release-heavy days: `cargo clean` between major
   version trains, or periodically `df -h` before starting a build.

## Rule of thumb

Linker Bus Error or rustc SIGSEGV mid-link/mid-check on a big workspace =
no space left on device until proven otherwise by `df -h`. Chasing the
crash as a code bug first wastes hours.

## Provenance

Born 2026-08-18 in the Agent Vesper workspace: rustc SIGSEGV during
xtask verify; df showed 100% usage; cargo clean freed 53.1 GiB and the
identical gate went green.
