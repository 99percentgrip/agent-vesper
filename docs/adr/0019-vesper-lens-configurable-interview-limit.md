# ADR 0019 — Configurable VesperLens Interview Limit

- **Status:** Accepted
- **Date:** 2026-08-17
- **Builds on:** ADR 0018

## Context

ADR 0018 capped every planning interview at four questions. Four is a useful
default for ordinary ambiguity, but a fixed ceiling loses important decisions
in larger PRDs and encourages repeated browser rounds. Removing the bound
entirely would let a model produce an overwhelming or accidental interview.

## Decision

1. `/interview-limit` with no argument reports the active session policy.
2. `/interview-limit 1` through `/interview-limit 12` sets a hard maximum.
   The agent may ask fewer questions when the requirements are already clear.
3. `/interview-limit auto` lets the agent choose only the unresolved,
   decision-relevant questions needed for the current PRD, bounded to 1–12.
4. New sessions default to a fixed maximum of four for compatibility.
5. The active policy is projected into a fresh `request_human_input` tool
   schema for every turn and independently enforced by the executor.

## Boundaries

- Twelve is the safety ceiling in both fixed and auto modes.
- The policy is session-scoped and is not persisted as a global preference.
- A fixed value is a maximum, never a quota; the model must not pad an
  interview merely to reach it.
- Each question still supports at most six options and every rendered question
  must be answered before submission.
- ADR 0017 and ADR 0018 network, escaping, browser, and feedback constraints
  remain unchanged. This decision supersedes only ADR 0018's fixed cap of four.

## Consequences

- Small requests retain the concise four-question default.
- Complex PRDs can use one larger interview or let the agent size it
  automatically without removing deterministic bounds.
- The slash-command status, palette choices, advertised JSON schema, and
  runtime validation cannot silently disagree.

## Verification

- `cargo test -p agent-vesper-tui --lib`
- `cargo test -p agent-vesper-tui --bin agent-vesper-tui`
- `cargo clippy -p agent-vesper-tui --all-targets --all-features -- -D warnings`
- `cargo xtask architecture`
