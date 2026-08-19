---
name: verify-parity-end-to-end
description: Prove cross-frontend parity (ACP vs TUI) per advertised command through the REAL shipped artifact before claiming it; shared semantics live in ONE implementation both hosts delegate to. Learned 2026-08-19 from the v0.20.54 failure where ACP /skills answered "No learned skills." while the TUI listed them.
version: 1.0.0
author: Agent Vesper library (learned from field failure v0.20.54 → fixed v0.20.55)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [parity, acp, tui, slash-commands, verification, e2e, drift]
prerequisites:
  commands: [cargo]
---

# Verify Parity End-to-End Before Claiming It

Saying "full parity with the TUI harness" while 15 of 28 advertised slash
commands answered a stub and `/skills` missed the global skill layer cost
hours and was caught by the user in production, not by our tests.

## When to apply

- Claiming parity between two frontends over one shared catalog (ACP ↔ TUI,
  CLI ↔ UI, plugin ↔ app).
- Adding/renaming commands or store layers in either composition.
- Opening shared durable stores from a new host.

## Rules

1. **ONE shared implementation.** If two hosts need identical behavior
   (store-open semantics, command execution), it lives in the shared crate
   (`vesper-harness`) and BOTH hosts delegate. Parallel private copies
   (`MemoryStores::open_default` in TUI `main.rs` vs harness) WILL drift.
2. **Every advertised name needs a real match arm** in the shared executor.
   Falling through to `_ => Unknown` prints "Unknown command" for commands
   you actively advertise (`/curator` had exactly this bug).
3. **Prove each surface through the real shipped artifact**: spawn the
   actual binary, drive its wire protocol (ACP JSON-RPC over stdio), assert
   the user-visible output. In-process unit tests on internals cannot
   catch wiring gaps — the binary is the composition.
4. **Grep shipped text before declaring done**: stub markers
   ("not available yet", "deferred", "TODO") over README/AGENTS/docs must
   return 0 for the surface you claim; update docs in the same commit.
5. **Append-then-rewrite persistence**: when a store appends a record and
   then atomically rewrites the log, the rewrite buffer must include the
   just-appended record — otherwise the rewrite clobbers it and only fresh
   reopens lose data (in-memory state masks the bug; see
   `SessionLineage::create` pre-v0.20.55).
6. **Verify CI on the final HEAD after every push**, including docs-only
   pushes; "the code commit passed" is not "HEAD passes".

## Failure signature (do not repeat)

- Symptom: ACP clients showed `No learned skills.`; the TUI listed skills.
- Root cause 1: the TUI's private `open_default` used
  `SkillStore::open_with_global`; the harness copy ACP used did not.
- Root cause 2: 15 host-owned commands returned a "not available yet" stub
  while docs claimed parity.
- Root cause 3: `SessionLineage::create` rewrite clobbered its own append.

## Fastest proof recipe

Drive the real `agent-vesper-acp` binary over stdio and assert the
user-visible output of every advertised command (see the
`host_owned_slash_commands_reach_real_stores_with_tui_parity` process test
in `apps/agent-vesper-acp/tests/process_transcript.rs`): initialize →
session/new → session/prompt `/skills`, then assert the response text lists
real skills. If the skill list — or any advertised command's real output —
is missing from stdout, parity is NOT done, regardless of green unit tests.
