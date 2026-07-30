# ADR 0001: Vesper State Location and Legacy Import

Status: RECOMMENDED

## Context

Python uses historical `.glm-acp` and platform config roots
(`config.py:685-710`, `profiles.py:13-24`, `session_store.py:24-69`).
Silent reuse would risk legacy corruption and complicate rollback.

## Decision

Use a new Agent Vesper state root. Discover legacy state read-only, expose an
explicit dry-run import/migration, keep a hash manifest and backup, and never
overwrite or delete legacy state automatically. Coexistence remains valid.

## Consequences

Legacy readers are mandatory; dual-write is forbidden absent a transaction
design. Exact root naming and migration UX require user approval.

