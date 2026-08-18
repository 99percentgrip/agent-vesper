---
name: harness-skill-gate-mechanics
description: How to satisfy the agent harness skill-learning gate — learn_skill refuses ("verify the outcome first") until the current task contains an edit followed by a canonical verification command. Piped/compound test commands do NOT register. Use when creating or updating project skills.
version: 1.1.0
author: Agent Vesper library (migrated from legacy GLM-ACP)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [skill-learning, harness, verification, meta]
---

# Harness Skill Gate Mechanics

When learn_skill fails with "may be learned only after a successful
verification command in the current task":

1. The gate requires BOTH, in order, within the current task: (a) an edit
   via write_file/edit_file, then (b) a canonical verification command that
   exits 0.
2. Canonical means VERBATIM cargo invocation with NO pipes:
   `cargo test -p <crate> --lib <filter>` or `cargo check`. Wrapping cargo
   in `grep | awk` pipelines (even with overall exit 0) does NOT register
   as verification.
3. For read-only mandates (learn/audit turns where coding is forbidden),
   the legitimate edit is the learning artifact itself: write the
   post-mortem/evidence doc + DOX index update, then run the narrow
   verbatim test, then call learn_skill immediately.
4. Never let the gate defer or cancel a direct order — satisfy the servant
   mechanics and deliver in the same turn.

## Provenance

Learned in the legacy GLM-ACP agent on 2026-08-17; migrated to the Agent
Vesper library.
