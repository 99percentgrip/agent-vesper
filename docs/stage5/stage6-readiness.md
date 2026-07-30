# Stage 6 Readiness

Status: COMPLETE — CI VALIDATION PENDING

## Decision

Stage 5 is locally ready to hand off its read-only contracts. The complete
workspace, Rust 1.88 MSRV, architecture, supply-chain, stability, and invariance
suite passes. Four non-Linux-x86-64 target families remain remote-CI pending
and must not be described as validated.

## Stage 5 handoff

Stage 6 receives:

- independent Agent Vesper and legacy read-store layouts;
- bounded filesystem enumeration and decoding;
- typed read outcomes and explicit unsupported mutations;
- deterministic collision precedence;
- schema-v1 compatibility preservation;
- future Agent Vesper format-v1 read decoder;
- pure runtime-state conversion;
- stable compatibility message identities;
- writer-acknowledged safe replay;
- keyed concurrent-load consistency;
- real-process disk-invariance tests;
- testkit store builders and hash/no-write assertions;
- architecture and dependency gates forbidding a writer or SQLite in Stage 5.

## Exact next target

Stage 6 — transactional Agent Vesper session writes, revisions, crash safety, and derived metadata

## Mandatory Stage 6 constraints

- Legacy Native GLM ACP state remains read-only.
- Writes target only the independent Agent Vesper state root.
- Revision/conflict semantics precede write exposure.
- Temporary-file creation, flush, atomic replacement, directory durability,
  permissions/ACLs, and crash recovery require real platform tests.
- Derived metadata must be rebuildable and must not contain reasoning, system
  prompts, secrets, or unredacted private content.
- No silent legacy migration, overwrite, or repair.
- SQLite/FTS remains outside Stage 6 unless separately authorized.
- Existing disk-invariance vectors remain regression tests for all legacy
  sources.

## Pending external evidence

- Linux ARM64
- macOS Intel
- macOS Apple Silicon
- Windows x86-64

Workflow jobs are prepared; no pending runner result is claimed as passing.
