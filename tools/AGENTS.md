# Migration Tools

## Purpose

Own non-production tooling used to capture and validate migration evidence.

## Ownership

- `python-oracle/` owns the isolated frozen-source fixture capture runner.

## Local Contracts

- Tools must never write to the frozen source or real user state.
- Tool stdout is machine-readable where documented; diagnostics are sanitized.
- Every subprocess is bounded and cleaned up.

## Work Guidance

- Keep dependencies explicit and prefer standard-library orchestration.

## Verification

- Run the tool's self-test/validation commands documented by its child DOX.

## Child DOX Index

- `python-oracle/AGENTS.md` — source-worker isolation and fixture capture rules.
