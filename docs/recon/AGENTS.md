# Migration Reconnaissance

## Purpose

Own the evidence-backed inventory, behavioral contract, architecture proposal, compatibility analysis, risk register, and staged migration plan for rebuilding Native GLM ACP as Rust-native Agent Vesper.

## Ownership

- `evidence-index.md` is the durable inspection ledger and command log.
- The remaining Markdown reports are the mission deliverables named by the user.

## Local Contracts

- The source repository at `/home/alex/Projects/Native GLM-5.2 Provider` is immutable.
- Reconnaissance is planning-only: do not add Rust crates, production code, speculative dependencies, or copied Python modules.
- Every material current-state claim cites a source path and symbol; include a test citation when one exists.
- Label behavior as confirmed, inferred, ambiguous, or untested.
- Tie every dependency recommendation to an observed requirement and current documentation.
- Mark each report `Status: IN PROGRESS`, `Status: COMPLETE`, or `Status: BLOCKED`.

## Work Guidance

- Inspect bounded subsystems, record evidence immediately, and reread the evidence index before each phase.
- Prefer symbol searches and focused line ranges over whole-file dumps.
- Use repository-relative paths in evidence citations and identify the source repository once per document.

## Verification

- Check all required reports exist, contain a status marker, distinguish proposals from facts, and have no placeholders or unsupported completion claims.
- Confirm source Git status and commit are unchanged at closeout.
- Confirm no Rust production files exist in the target.

## Child DOX Index

No children.
